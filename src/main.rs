extern crate rand;
extern crate time;
extern crate getopts;
extern crate mersenne_twister;

use getopts::Options;
use mersenne_twister::MersenneTwister;
use rand::{Rng, SeedableRng, random};
use std::env;
use std::fs::{OpenOptions, File, metadata};
use std::io::{Read, Write};
use std::iter::repeat;
use std::process::exit;
use std::str::FromStr;
use std::string::String;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::thread;
use std::thread::sleep_ms;
use time::{Duration, SteadyTime};

#[derive(Clone, Debug)]
struct Config {
  threads    :i32,
  framerate  :f32,
  framesize  :usize,
  timelimit  :Duration,
  workdir    :String,
  hostname   :String,
}

fn main() {
  let config  = opts();
  let mut ts  = Vec::new();
  let thcount = config.threads;
  let mut all = true;

  for i in 0..thcount {
    all = verify_workfile(&config, i) && all;
  }
  if !all {
    println!("Created work files. quitting.");
    return;
  }

  for i in 0..thcount {
    let conf = config.clone();
    ts.push(thread::spawn(move || {play(&conf, i);}));
  }

  loop {
    match ts.pop() {
      None => return,
      Some(handle) => {handle.join(); ()},
    }
  }
}

/// Parse argv options into a configuration object. This will panic if the
/// given argv cannot be understood, and will give a configuration otherwise.
fn opts() -> Config {
  let args: Vec<String> = env::args().collect();
  let program = args[0].clone();
  let mut opts = Options::new();
  opts.optflag("h", "help", "print this help text");
  opts.optopt("t", "threads", "set thread count", "NUM");
  opts.optopt("o", "host", "set hostname", "HOST");
  opts.optopt("d", "dir", "set working directory", "DIR");
  opts.optopt("r", "rate", "set code frame rate", "RATE");
  opts.optopt("s", "size", "set code frame size", "SIZE");
  opts.optopt("l", "limit", "set time limit", "SECONDS");
  let matches = match opts.parse(&args[1..]) {
    Ok(m) => { m }
    Err(f) => { panic!(f.to_string()) }
  };

  if matches.opt_present("h") {
    let brief = format!("Usage: {} [options]", program);
    print!("{}", opts.usage(&brief));
    exit(0);
  };

  let threads = match matches.opt_str("t") {
    None      => {1}
    Some(t) => {FromStr::from_str(&t).unwrap()} };

  let host = match matches.opt_str("o") {
    None      => {String::from("localhost")}
    Some(h) => {String::from(h)} };

  let dir = match matches.opt_str("d") {
    None      => {String::from(".")}
    Some(d) => {String::from(d)} };

  let rate = match matches.opt_str("r") {
    None      => {24.0}
    Some(r) => {FromStr::from_str(&r).unwrap()} };

  let size = match matches.opt_str("s") {
    None      => {1024*1024}
    Some(s) => {FromStr::from_str(&s).unwrap()} };

  let sec = match matches.opt_str("l") {
    None      => {8*60}
    Some(s) => {FromStr::from_str(&s).unwrap()} };

  Config {
    threads:    threads,
    framerate:  rate,
    framesize:  size,
    timelimit:  Duration::seconds(sec),
    workdir:    dir,
    hostname:   host,
    }
}

/// Ensure that the working files are all present. Returns True if everything
/// was already there, False if some files had to be written (or expanded).
/// Main will exit if any files had to be written, which makes aligning
/// multi-machine benchmarks much easier.
fn verify_workfile(config: &Config, threadno: i32) -> bool {
  let name = workfile_name(&config, threadno);
  println!("Verifying existence of {}", name);
  let desired_sz = (config.framerate.ceil() as usize) *
                   config.framesize *
                   (config.timelimit.num_seconds() as usize + 1);
  let mut sofar = match metadata(&name) {
    Err(_)   => {0}
    Ok(meta) => {meta.len() as usize} };

  if sofar < desired_sz {
    let mut fd = OpenOptions::new()
      .write(true)
      .create(true)
      .append(true)
      .open(&name).unwrap();
    let mut buf:Vec<u8> = repeat(0).take(1024*1024).collect();
    let rand : u64 = random();
    let mut rng : MersenneTwister = SeedableRng::from_seed(rand);
    while sofar < desired_sz {
      rng.fill_bytes(&mut buf);
      sofar += fd.write(&buf).unwrap();
    }
    return false;
  }
  true
}

/// Generate the name that this thread will use for its work file
fn workfile_name(config: &Config, threadno: i32) -> String {
  let mut path = String::new();
  path.push_str(&config.workdir[..]);
  path.push_str("/vio-work-");
  path.push_str(&config.hostname[..]);
  path.push_str("-");
  let threadstr = format!("{}", threadno);
  path.push_str(&threadstr);
  path
}

struct Buffered {
  local: usize,
  chan:  Receiver<usize>,
}

/// Simulate playing a video. This will run through the work file at the
/// configured framerate and frame size, logging every time that a frame could
/// not be delivered on time. The final result of this function is a message
/// that displays the total number of frames that were "played", and how many
/// had to be dropped.
fn play(config: &Config, threadno: i32) {
  let path            = workfile_name(config, threadno);
  let mut total       = 0;
  let mut fails       = 0;
  let frame_len       = Duration::microseconds( (1e6 / config.framerate) as i64);
  let start           = SteadyTime::now();
  let end_time        = start + config.timelimit;
  let mut frame_end   = start + frame_len;
  let (tx, rx)        = sync_channel(8);
  let mut buffered    = Buffered { local: 0, chan: rx };

  thread::spawn(move || { read_file(tx, path) });

  loop {
    total += 1;
    if frame(&mut buffered, config.framesize, &frame_end, &mut fails) {
      report(total, fails);
      return;
    }
    if SteadyTime::now() > end_time {
      report(total, fails);
      return;
    }
    frame_end   = frame_end + frame_len;
  }
}

fn report(total: i32, fails: i32) {
  let percent = 100.0 * (fails as f32) / (total as f32);
  println!("{} frames, {} failures ({}%)", total, fails, percent);
}

/// Reads the entire file, writing to the channel the amount that it's read.
/// The frame function will feed from the associated channel when it needs more
/// data to "play".
fn read_file(tx: SyncSender<usize>, path: String) {
  let mut file         = File::open(path).unwrap();
  let mut buf: Vec<u8> = repeat(0).take(4*1024*1024).collect();
  loop {
    let read = file.read(&mut buf).unwrap();
    if read <= 0 {
      return;
    }
    tx.send(read).unwrap();
  }
}

/// Read the desired amount of data from the buffered file. Returns True if the
/// file hit EOF, False if there's more to read.
fn read_buffer(buffered: &mut Buffered, mut amount: usize) -> bool {
  if buffered.local > amount {
    buffered.local -= amount;
    false
  }
  else {
    amount -= buffered.local;
    match buffered.chan.recv() {
      Ok(more) => { buffered.local = more; read_buffer(buffered, amount) }
      Err(_)   => { true }
    }
  }
}

/// Play a frame. This takes the time at which the frame needs to be completed.
/// If the function is called after that time (because a previous frame was
/// seriously delayed) it will fail immediately and log the failure. If this
/// frame takes too long, the failure will be logged. If the frame gets loaded
/// before the cutoff time, this function will sleep until the frame is done
/// being shown.
///
/// This returns True if the file reaches EOF, false if there's more to be read.
fn frame(buffered:  &mut Buffered,
         frame_sz:  usize,
         frame_end: &SteadyTime,
         fails:     &mut i32
        ) -> bool {
  if SteadyTime::now() > *frame_end {
    *fails += 1;
    return false;
  }

  let eof = read_buffer(buffered, frame_sz);
  if SteadyTime::now() > *frame_end {
    *fails += 1;
    return eof;
  }
  let delay = (*frame_end - SteadyTime::now()).num_milliseconds();
  if delay > 0 {
    sleep_ms(delay as u32);
  }
  eof
}

