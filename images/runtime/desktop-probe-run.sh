#!/bin/sh
# The desktop-probe image's run (test image only): RT-I-046's browser cases, then the AT-SPI and set-of-marks cases,
# each probe whatever the one before did, so a failing browser case still leaves the AT-SPI results (c17: RT-I-047
# went unrun behind RT-I-046's two failures). The clipboard case runs last, alone: its paste in leaves x11vnc owning
# PRIMARY and CLIPBOARD, and while it does a LibreOffice double-click fails about 1 time in 3 (c17's trials: 5 of 15
# after a paste, 0 of 14 with the selections cleared). A known limit in c17; c18 hands a paste-in to an owner that
# answers at once, so applications never wait on x11vnc's selection (RT-I-058). Fails if any of the three failed.
args="--ignored --test-threads=1 --nocapture"
paste=rt_i_046_nothing_is_copied_out_and_a_paste_reaches_the_x_clipboard
computer-browser-probe $args
browser=$?
computer-desktop-probe $args --skip "$paste"
desktop=$?
computer-desktop-probe $args --exact "$paste"
clipboard=$?
[ "$browser" -eq 0 ] && [ "$desktop" -eq 0 ] && [ "$clipboard" -eq 0 ]
