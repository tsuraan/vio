# Video I/O Benchmark

This is a small tool that helps determine how many video streams your storage
can serve. Its three main arguments are the size of a frame, how many frames
per second the codec demands, and how many simultaneous threads to run. It
will initially create a pseudo-random "video" file for each thread and exit. A
second invocation will have each thread read from its file at a rate of one
frame's worth of data per time interval. It records each time that a frame
could not be fully read in the required time span, and upon completion, prints
a very simple report.

## Building

```
cargo build
```

## Usage

```
cargo run -- -h
Usage: target/debug/vio [options]

Options:
    -h, --help          print this help text
    -t, --threads NUM   set thread count
    -o, --host HOST     set hostname
    -d, --dir DIR       set working directory
    -r, --rate RATE     set code frame rate
    -s, --size SIZE     set code frame size
    -l, --limit SECONDS set time limit
```

## Future Work

vio doesn't do any caching of its own, nor any background reading. I'm fairly
certain that any sane video player will do this, so vio should as well. Better
reporting (recording actual latencies for each frame, etc) would probably also
be good.
