#
# This file is part of Cynthion.
#
# Copyright (c) 2024 Great Scott Gadgets <info@greatscottgadgets.com>
# SPDX-License-Identifier: BSD-3-Clause

from amaranth import Elaboratable, Module, Signal, Cat, Mux
from amaranth.lib.fifo import SyncFIFO

from luna.gateware.stream import StreamInterface


class StreamFIFO(Elaboratable):
    def __init__(self, fifo):
        self.fifo   = fifo
        self.input  = StreamInterface(payload_width=fifo.width)
        self.output = StreamInterface(payload_width=fifo.width)

    def elaborate(self, platform):
        m = Module()

        m.submodules.fifo = self.fifo

        m.d.comb += [
            self.fifo.w_data    .eq(self.input.payload),
            self.fifo.w_en      .eq(self.input.valid),
            self.input.ready    .eq(self.fifo.w_rdy),

            self.output.payload .eq(self.fifo.r_data),
            self.output.valid   .eq(self.fifo.r_rdy),
            self.fifo.r_en      .eq(self.output.ready),
        ]

        return m


class Stream16to8(Elaboratable):
    def __init__(self, msb_first=True):
        self.msb_first = msb_first
        self.input     = StreamInterface(payload_width=16)
        self.output    = StreamInterface(payload_width=8)

    def elaborate(self, platform):
        m = Module()

        input_data = self.input.payload
        if self.msb_first:
            input_data = Cat(input_data[8:16], input_data[0:8])

        odd_byte   = Signal()
        data_shift = Signal.like(self.input.payload)  # shift register
        m.d.comb  += self.output.payload.eq(data_shift[0:8])

        # When the output stream is not stalled...
        with m.If(self.output.ready | ~self.output.valid):

            # If odd_byte is asserted, send the buffered second byte
            with m.If(odd_byte):
                m.d.sync += [
                    data_shift          .eq(data_shift[8:]),
                    self.output.valid   .eq(1),
                    odd_byte            .eq(0),
                ]

            # Otherwise, consume a new word from the input stream
            with m.Else():
                m.d.comb += self.input.ready .eq(1)
                m.d.sync += self.output.valid.eq(self.input.valid)
                with m.If(self.input.valid):
                    m.d.sync += [
                        data_shift .eq(input_data),
                        odd_byte   .eq(1),
                    ]

        return m


class StreamSkidBuffer(Elaboratable):
    def __init__(self, payload_width, reg_output=False):
        self.input      = StreamInterface(payload_width)
        self.output     = StreamInterface(payload_width)
        self.reg_output = reg_output

    def elaborate(self, platform):
        m = Module()

        # Internal signals.
        r_valid     = Signal()
        in_payload  = Cat(self.input.payload, self.input.last)
        out_payload = Cat(self.output.payload, self.output.last)
        r_payload   = Signal.like(in_payload, reset_less=True)

        # Internal storage is only valid when there is incoming
        # data but the consumer is not ready.
        with m.If((self.input.ready & self.input.valid) & (self.output.valid & ~self.output.ready)):
            m.d.sync += r_valid.eq(1)
        with m.Elif(self.output.ready):
            m.d.sync += r_valid.eq(0)

        # Keep storing input data.
        with m.If(self.input.ready & self.input.valid):
            m.d.sync += r_payload.eq(in_payload)
        
        # As long as our internal buffer is empty, we accept a new sample
        # This internal buffer provides the "elasticity" needed due to
        # the register delay in the `ready` signal path.
        m.d.comb += self.input.ready.eq(~r_valid)

        # Drive output valid and data signals.
        out_domain = m.d.comb if not self.reg_output else m.d.sync
        out_domain += self.output.valid.eq(self.input.valid | r_valid)
        out_domain += out_payload.eq(Mux(r_valid, r_payload, in_payload))

        return m
