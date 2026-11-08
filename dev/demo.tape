Output docs/images/sb-history.gif

Set Shell "bash"
Set FontFamily "JetBrains Mono"
Set FontSize 18
Set Width 1200
Set Height 720
Set Padding 24
Set Margin 24
Set MarginFill "#1e1e2e"
Set BorderRadius 10
Set Theme "Catppuccin Mocha"
Set Framerate 15
Set TypingSpeed 40ms
Set CursorBlink false

# Build and setup are not part of the recording.
Hide
Type "unset CARGO_TARGET_DIR"
Enter
Type "cargo build -q -p sb-daemon --features test-hwmon && cargo build -q -p sb"
Enter
Sleep 25s
Type "./dev/harness.sh --db $SB_DEMO_DB --static --tui"
Enter
Wait+Screen /CPU Usage/
Sleep 3s
Show

# The history views are real data from the supplied DB. Skip the live view:
# in static mode it is a flat snapshot, while the 10 s/1 m/10 m/1 h views carry
# the actual fan and temperature history.
Type "t"
Sleep 4s
Type "t"
Sleep 3s
Screenshot docs/images/sb-history.png
Sleep 1s
Type "t"
Sleep 3s
Type "t"
Sleep 2s
# Scroll back to the Oct 7 14:00 bucket where PWM jumps 130 -> 255 and the
# fan reaches ~2966 RPM. 123 rapid presses move 721 hourly rows.
Left@25ms 123
Sleep 3s
Hide
Type "q"
Sleep 1s
