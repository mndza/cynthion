#
# This file is part of Cynthion.
#
# Copyright (c) 2024 Great Scott Gadgets <info@greatscottgadgets.com>
# SPDX-License-Identifier: BSD-3-Clause

from enum import IntEnum


class USBAnalyzerEvent(IntEnum):
    NONE               = 0
    CAPTURE_STOP       = 1
    CAPTURE_FULL       = 2
    CAPTURE_RESUME     = 3

    CAPTURE_START_BASE = 4

    CAPTURE_START_HIGH = 4
    CAPTURE_START_FULL = 5
    CAPTURE_START_LOW  = 6
    CAPTURE_START_AUTO = 7

    SPEED_DETECT_BASE  = 8

    SPEED_DETECT_HIGH  = 8
    SPEED_DETECT_FULL  = 9
    SPEED_DETECT_LOW   = 10

    VBUS_CONNECTED     = 12
    VBUS_DISCONNECTED  = 13
    BUS_RESET          = 14
    SUSPEND_STARTED    = 15
    SUSPEND_ENDED      = 16
    DEVICE_CHIRP_SEEN  = 17
    HOST_CHIRP_SEEN    = 18
