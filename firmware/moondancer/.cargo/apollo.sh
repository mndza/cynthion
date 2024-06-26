#!/usr/bin/env zsh

# configuration
# TODO support flash offset address
: ${FLASH_OFFSET:=0x00000000}
: ${BITSTREAM:=../../cynthion/python/build/top.bit}
: ${UART:=/dev/ttyACM0}

echo "Using flash address: FLASH_OFFSET=$FLASH_OFFSET"
echo "Using bitstream: BITSTREAM=$BITSTREAM"
echo "Using uart: UART=$UART"

# convert ELF executable to bin image
echo "Creating firmware image: $1.bin"
NAME=$(basename $1)
cargo objcopy --release --bin $NAME -- -Obinary $1.bin

# flash firmware to cynthion
echo "Flashing firmware image: $1.bin"
apollo flash --offset $FLASH_OFFSET $1.bin

# configure cynthion with soc bitstream
echo "Configuring fpga: $BITSTREAM"
apollo configure $BITSTREAM 2>/dev/null

# start a terminal for debug output
pyserial-miniterm $UART 115200
