#![allow(dead_code, unused_imports, unused_mut, unused_variables)] // TODO
#![no_std]
#![no_main]

use heapless::mpmc::MpMcQueue as Queue;
use log::{debug, error, info, trace, warn};

use smolusb::control::{Control, Descriptors};
use smolusb::device::Speed;

use smolusb::setup::{Direction, RequestType, SetupPacket};
use smolusb::traits::{UsbDriverOperations, WriteEndpoint};

use ladybug::{Bit, Channel};

use libgreat::gcp::{GreatResponse, LIBGREAT_MAX_COMMAND_SIZE};
use libgreat::{GreatError, GreatResult};

use moondancer::event::InterruptEvent;
use moondancer::usb::vendor::{VendorRequest, VendorValue};
use moondancer::{hal, pac};

use pac::csr::interrupt;

// - configuration ------------------------------------------------------------

const DEVICE_SPEED: Speed = Speed::High;

// - MachineExternal interrupt handler ----------------------------------------

static EVENT_QUEUE: Queue<InterruptEvent, 64> = Queue::new();

#[inline(always)]
fn dispatch_event(event: InterruptEvent) {
    match EVENT_QUEUE.enqueue(event) {
        Ok(()) => (),
        Err(_) => {
            error!("MachineExternal - event queue overflow");
            loop {
                unsafe {
                    riscv::asm::nop();
                }
            }
        }
    }
}

#[allow(non_snake_case)]
#[no_mangle]
fn MachineExternal() {
    match moondancer::util::get_usb_interrupt_event() {
        InterruptEvent::UnhandledInterrupt(pending) => {
            dispatch_event(InterruptEvent::UnknownInterrupt(pending));
        }
        event => {
            dispatch_event(event);
        }
    }
}

// - main entry point ---------------------------------------------------------

#[cfg(feature = "vexriscv")]
#[riscv_rt::pre_init]
unsafe fn pre_main() {
    pac::cpu::vexriscv::flush_icache();
    #[cfg(feature = "vexriscv_dcache")]
    pac::cpu::vexriscv::flush_dcache();
}

#[riscv_rt::entry]
fn main() -> ! {
    // initialize firmware
    let mut firmware = Firmware::new(pac::Peripherals::take().unwrap());
    match firmware.initialize() {
        Ok(()) => (),
        Err(e) => {
            panic!("Firmware panicked during initialization: {}", e)
        }
    }

    // enter main loop
    match firmware.main_loop() {
        Ok(()) => {
            panic!("Firmware exited unexpectedly in main loop")
        }
        Err(e) => {
            panic!("Firmware panicked in main loop: {}", e)
        }
    }
}

// - Firmware -----------------------------------------------------------------

struct Firmware<'a> {
    // peripherals
    leds: pac::LEDS,
    usb1: hal::Usb1,

    // state
    libgreat_response: Option<GreatResponse>,
    libgreat_response_last_error: Option<GreatError>,

    // classes
    core: libgreat::gcp::class_core::Core,
    moondancer: moondancer::gcp::moondancer::Moondancer,

    pub _marker: core::marker::PhantomData<&'a ()>,
}

// - lifecycle ----------------------------------------------------------------

impl<'a> Firmware<'a> {
    fn new(peripherals: pac::Peripherals) -> Self {
        // initialize logging
        moondancer::log::init(hal::Serial::new(peripherals.UART));
        info!(
            "{} {}",
            cynthion::shared::usb::bManufacturerString::cynthion,
            cynthion::shared::usb::bProductString::cynthion,
        );
        info!("Logging initialized");

        // initialize ladybug
        moondancer::debug::init(peripherals.GPIOA, peripherals.GPIOB);

        // usb1: aux (host on r0.4)
        let usb1 = hal::Usb1::new(
            peripherals.USB1,
            peripherals.USB1_EP_CONTROL,
            peripherals.USB1_EP_IN,
            peripherals.USB1_EP_OUT,
        );

        // usb0: target
        let usb0 = hal::Usb0::new(
            peripherals.USB0,
            peripherals.USB0_EP_CONTROL,
            peripherals.USB0_EP_IN,
            peripherals.USB0_EP_OUT,
        );

        // initialize libgreat class registry
        static CLASSES: [libgreat::gcp::Class; 4] = [
            libgreat::gcp::class_core::CLASS,
            moondancer::gcp::firmware::CLASS,
            moondancer::gcp::selftest::CLASS,
            moondancer::gcp::moondancer::CLASS,
        ];
        let classes = libgreat::gcp::Classes(&CLASSES);

        // initialize libgreat classes
        let core = libgreat::gcp::class_core::Core::new(classes, moondancer::BOARD_INFORMATION);
        let moondancer = moondancer::gcp::moondancer::Moondancer::new(usb0);

        Self {
            leds: peripherals.LEDS,
            usb1,
            //usb1_control,
            libgreat_response: None,
            libgreat_response_last_error: None,
            core,
            moondancer,
            _marker: core::marker::PhantomData,
        }
    }

    fn initialize(&mut self) -> GreatResult<()> {
        // leds: starting up
        self.leds
            .output
            .write(|w| unsafe { w.output().bits(1 << 2) });

        // connect usb1
        self.usb1.connect(DEVICE_SPEED);
        info!("Connected usb1 device");

        // enable interrupts
        unsafe {
            // set mstatus register: interrupt enable
            riscv::interrupt::enable();

            // set mie register: machine external interrupts enable
            riscv::register::mie::set_mext();

            // write csr: enable usb1 interrupts and events
            interrupt::enable(pac::Interrupt::USB1);
            interrupt::enable(pac::Interrupt::USB1_EP_CONTROL);
            interrupt::enable(pac::Interrupt::USB1_EP_IN);
            interrupt::enable(pac::Interrupt::USB1_EP_OUT);

            // enable all usb events
            self.usb1.enable_interrupts();
        }

        Ok(())
    }
}

// - main loop ----------------------------------------------------------------

impl<'a> Firmware<'a> {
    #[inline(always)]
    fn main_loop(&'a mut self) -> GreatResult<()> {
        let mut usb1_control = Control::<_, LIBGREAT_MAX_COMMAND_SIZE>::new(
            0,
            Descriptors {
                device_speed: DEVICE_SPEED,
                device_descriptor: moondancer::usb::DEVICE_DESCRIPTOR,
                configuration_descriptor: moondancer::usb::CONFIGURATION_DESCRIPTOR_0,
                other_speed_configuration_descriptor: Some(
                    moondancer::usb::OTHER_SPEED_CONFIGURATION_DESCRIPTOR_0,
                ),
                device_qualifier_descriptor: Some(moondancer::usb::DEVICE_QUALIFIER_DESCRIPTOR),
                string_descriptor_zero: moondancer::usb::STRING_DESCRIPTOR_0,
                string_descriptors: moondancer::usb::STRING_DESCRIPTORS,
            },
        );

        let mut max_queue_length: usize = 0;
        let mut queue_length: usize = 0;
        let mut counter: usize = 1;

        info!("Peripherals initialized, entering main loop");

        loop {
            // leds: main loop is responsive, interrupts are firing
            self.leds
                .output
                .write(|w| unsafe { w.output().bits((counter % 256) as u8) });

            if queue_length > max_queue_length {
                max_queue_length = queue_length;
                debug!("max_queue_length: {}", max_queue_length);
            }
            queue_length = 0;

            while let Some(interrupt_event) = EVENT_QUEUE.dequeue() {
                counter += 1;
                queue_length += 1;

                // leds: event loop is active
                self.leds
                    .output
                    .write(|w| unsafe { w.output().bits(1 << 0) });

                use moondancer::{
                    event::InterruptEvent::*,
                    UsbInterface::{Aux, Target},
                };
                use smolusb::event::UsbEvent::*;

                match interrupt_event {
                    // - misc event handlers --
                    ErrorMessage(message) => {
                        error!("MachineExternal Error - {}", message);
                    }

                    // - usb1 Aux event handlers --

                    // Usb1 received a control event
                    Usb(Aux, event @ BusReset)
                    | Usb(Aux, event @ ReceiveControl(0))
                    | Usb(Aux, event @ ReceiveSetupPacket(0, _))
                    | Usb(Aux, event @ ReceivePacket(0))
                    | Usb(Aux, event @ SendComplete(0)) => {
                        trace!("Usb(Aux, {:?})", event);

                        match usb1_control.handle_event(&self.usb1, event) {
                            // vendor requests are not handled by control
                            Some((setup_packet, rx_buffer)) => {
                                self.handle_vendor_request(setup_packet, rx_buffer)?
                            }
                            // control event was handled
                            None => (),
                        }
                    }

                    // - usb0 Target event handlers --

                    // enqueue moondancer events
                    Usb(Target, _event) => {
                        self.moondancer.dispatch_event(interrupt_event)
                    },

                    // Unhandled event
                    _ => {
                        error!("Unhandled event: {:?}", interrupt_event);
                    }
                }
            }
        }

        #[allow(unreachable_code)] // TODO
        Ok(())
    }
}

// - usb1 control handler -----------------------------------------------------

impl<'a> Firmware<'a> {
    /// Handle GCP vendor requests
    fn handle_vendor_request(
        &mut self,
        setup_packet: SetupPacket,
        rx_buffer: &[u8],
    ) -> GreatResult<()> {
        let direction = setup_packet.direction();
        let request_type = setup_packet.request_type();
        let vendor_request = VendorRequest::from(setup_packet.request);
        let vendor_value = VendorValue::from(setup_packet.value);

        debug!(
            "handle_vendor_request: {:?} {:?} {:?}",
            vendor_request, vendor_value, direction
        );

        match (&request_type, &vendor_request) {
            (RequestType::Vendor, VendorRequest::UsbCommandRequest) => {
                match (&vendor_value, &direction) {
                    // host is starting a new command sequence
                    (VendorValue::Execute, Direction::HostToDevice) => {
                        trace!("  GOT COMMAND data:{:?}", rx_buffer);
                        ladybug::trace(Channel::A, Bit::A_GCP_DISPATCH_REQUEST, || {
                            self.dispatch_libgreat_request(rx_buffer)
                        })?;
                    }

                    // host is ready to receive a response
                    (VendorValue::Execute, Direction::DeviceToHost) => {
                        trace!("  GOT RESPONSE REQUEST");
                        ladybug::trace(Channel::A, Bit::A_GCP_DISPATCH_RESPONSE, || {
                            self.dispatch_libgreat_response(&setup_packet)
                        })?;
                    }

                    // host would like to abort the current command sequence
                    (VendorValue::Cancel, Direction::DeviceToHost) => {
                        debug!("  GOT ABORT");
                        ladybug::trace(Channel::A, Bit::A_GCP_DISPATCH_ABORT, || {
                            self.dispatch_libgreat_abort(&setup_packet)
                        })?;
                    }

                    _ => {
                        error!(
                            "handle_vendor_request stall: unknown vendor request and/or value direction{:?} vendor_request{:?} vendor_value:{:?}",
                            direction, vendor_request, vendor_value
                        );
                        self.usb1.stall_control_request();
                    }
                }
            }
            (RequestType::Vendor, VendorRequest::Unknown(vendor_request)) => {
                error!(
                    "handle_vendor_request Unknown vendor request '{}'",
                    vendor_request
                );
                self.usb1.stall_control_request();
            }
            (RequestType::Vendor, vendor_request) => {
                // TODO this is from one of the legacy boards which we
                // need to support to get `greatfet info` to finish
                // enumerating through the supported devices.
                //
                // see: host/greatfet/boards/legacy.py

                // The greatfet board scan code expects the IN endpoint
                // to be stalled if this is not a legacy device.
                self.usb1.stall_endpoint_in(0);

                warn!(
                    "handle_control_event Legacy libgreat vendor request '{:?}'",
                    vendor_request
                );
            }
            _ => {
                error!(
                    "handle_control_event Unknown control packet '{:?}'",
                    setup_packet
                );
                self.usb1.stall_control_request();
            }
        }

        Ok(())
    }
}

// - libgreat command dispatch ------------------------------------------------

impl<'a> Firmware<'a> {
    fn dispatch_libgreat_request(&mut self, command_buffer: &[u8]) -> GreatResult<()> {
        // parse command
        let (class_id, verb_number, arguments) = match libgreat::gcp::Command::parse(command_buffer)
        {
            Some(command) => (command.class_id(), command.verb_number(), command.arguments),
            None => {
                error!("dispatch_libgreat_request failed to parse libgreat command");
                return Err(GreatError::BadMessage);
            }
        };

        debug!(
            "dispatch_libgreat_request {:?}.0x{:x}",
            class_id, verb_number
        );

        // dispatch command
        let response_buffer: [u8; LIBGREAT_MAX_COMMAND_SIZE] = [0; LIBGREAT_MAX_COMMAND_SIZE];
        let response = match class_id {
            // class: core
            libgreat::gcp::ClassId::core => {
                self.core.dispatch(verb_number, arguments, response_buffer)
            }
            // class: firmware
            libgreat::gcp::ClassId::firmware => {
                moondancer::gcp::firmware::dispatch(verb_number, arguments, response_buffer)
            }
            // class: selftest
            libgreat::gcp::ClassId::selftest => {
                moondancer::gcp::selftest::dispatch(verb_number, arguments, response_buffer)
            }
            // class: moondancer
            libgreat::gcp::ClassId::moondancer => {
                self.moondancer
                    .dispatch(verb_number, arguments, response_buffer)
            }
            // class: unsupported
            _ => {
                error!(
                    "dispatch_libgreat_request error: Class id '{:?}' not found",
                    class_id
                );
                Err(GreatError::InvalidArgument)
            }
        };

        // queue response
        match response {
            Ok(response) => {
                self.libgreat_response = Some(response);
                self.libgreat_response_last_error = None;
            }
            Err(e) => {
                error!(
                    "dispatch_libgreat_request error: failed to dispatch command {}",
                    e
                );
                self.libgreat_response = None;
                self.libgreat_response_last_error = Some(e);

                // TODO this is... weird...
                self.usb1.stall_endpoint_in(0);
                unsafe {
                    riscv::asm::delay(2000);
                }
                self.usb1.ep_in.reset.write(|w| w.reset().bit(true));
            }
        }

        Ok(())
    }

    fn dispatch_libgreat_response(&mut self, _setup_packet: &SetupPacket) -> GreatResult<()> {
        use smolusb::traits::UnsafeUsbDriverOperations;

        // do we have a response ready?
        if let Some(response) = &mut self.libgreat_response {
            // send response
            let bytes_written = self.usb1.write(0, response);

            // TODO should we block here?

            // clear cached response
            self.libgreat_response = None;

            // prime to receive host zlp - aka ep_out_prime_receive() TODO should control do this in send_complete?
            self.usb1.ack(0, Direction::DeviceToHost);

            debug!("dispatch_libgreat_response -> {} bytes", bytes_written);

        } else if let Some(error) = self.libgreat_response_last_error {
            warn!("dispatch_libgreat_response error result: {:?}", error);

            // write error
            self.usb1.write(0, (error as u32).to_le_bytes().into_iter());

            // clear cached error
            self.libgreat_response_last_error = None;

            // prime to receive host zlp - TODO should control do this in send_complete?
            self.usb1.ack(0, Direction::DeviceToHost);
        } else {
            // TODO figure out what to do if we don't have a response or error
            error!("dispatch_libgreat_response stall: libgreat response requested but no response or error queued");
            self.usb1.stall_control_request();
        }

        Ok(())
    }

    fn dispatch_libgreat_abort(&mut self, _setup_packet: &SetupPacket) -> GreatResult<()> {
        error!("dispatch_libgreat_response abort");

        // cancel any queued response
        self.libgreat_response = None;
        self.libgreat_response_last_error = None;

        // TODO figure out if the host is expecting a response
        /*self.usb1
        .write(0, [].into_iter());*/

        Ok(())
    }
}
