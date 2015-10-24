extern crate rand;
extern crate time;
extern crate getopts;
extern crate mersenne_twister;

use mersenne_twister::MersenneTwister;
use rand::{Rng, SeedableRng, random};
use std::process::exit;
use std::env;
use std::thread;
use std::ops::Sub;
use std::fs::{OpenOptions, File, metadata};
use std::str::FromStr;
use std::string::String;
use std::io::{Read, Write};
use std::ops::Rem;
use std::iter::repeat;
use std::thread::sleep_ms;
use time::{Duration, PreciseTime};
use getopts::Options;

#[derive(Clone, Debug)]
struct Config {
  threads    :i32,
  framerate  :f32,
  framesize  :usize,
  buffersize :usize,
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
    ts.push(thread::spawn(move || {frames(&conf, i, "/dev/urandom");}));
  }

  loop {
    match ts.pop() {
      None => return,
      Some(handle) => {handle.join(); ()},
    }
  }
}

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
  opts.optopt("b", "buffer", "set buffer size", "BUF");
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

  let buf = match matches.opt_str("b") {
    None      => {64*1024}
    Some(b) => {FromStr::from_str(&b).unwrap()} };

  let sec = match matches.opt_str("l") {
    None      => {8*60}
    Some(s) => {FromStr::from_str(&s).unwrap()} };

  Config {
    threads:    threads,
    framerate:  rate,
    framesize:  size,
    buffersize: buf,
    timelimit:  Duration::seconds(sec),
    workdir:    dir,
    hostname:   host,
    }
}

fn verify_workfile(config: &Config, threadno: i32) -> bool {
  let name = workfile_name(&config, threadno);
  println!("Verifying existence of {}", name);
  let desired_sz = (config.framerate.ceil() as usize) *
                   config.framesize *
                   (config.timelimit.num_seconds() as usize + 1);
  let mut to_write = match metadata(&name) {
    Err(_)   => {desired_sz}
    Ok(meta) => {desired_sz - meta.len() as usize} };

  if to_write > 0 {
    let mut fd = OpenOptions::new()
      .write(true)
      .create(true)
      .append(true)
      .open(&name).unwrap();
    let mut buf :[u8; 8192] = [0; 8192];
    let rand : u64 = random();
    let mut rng : MersenneTwister = SeedableRng::from_seed(rand);
    while to_write > 0 {
      rng.fill_bytes(&mut buf);
      to_write -= fd.write(&buf).unwrap();
    }
    return false;
  }
  true
}

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

fn frames(config: &Config, threadno: i32, path: &str) {
  let mut file        = File::open(path).unwrap();
  let mut buf:Vec<u8> = repeat(0).take(config.buffersize).collect();
  let mut total       = 0;
  let mut fails       = 0;
  let start           = PreciseTime::now();

  loop {
    if one_second(config, &mut file, &mut buf, &mut total, &mut fails) {
      println!("{} frames, {} failures", total, fails);
      return;
    }
    if start.to(PreciseTime::now()) > config.timelimit {
      println!("{} frames, {} failures", total, fails);
      return;
    }
  }
}

fn one_second(config:   &Config,
              mut file: &mut File,
              buf:      &mut[u8],
              total:    &mut i32,
              fails:    &mut i32
             ) -> bool
{
  let mut frameno = 0;
  let second_start = PreciseTime::now();
  loop {
    let sz  = frame_size(frameno, config.framerate, config.framesize);
    let dur = frame_duration(frameno, config.framerate, second_start);
    // println!("frameno is {}, sz is {}, dur is {}", frameno, sz, dur);
    let eof = frame(&mut file, buf, sz, dur, fails);
    *total += 1;
    // println!("eof {}", eof);
    if eof {
      return true;
    }
    frameno = next_frameno(frameno, config.framerate);
    if frameno == 0 {
      return false;
    }
  }
}

fn frame_size(frameno: i32, rate: f32, perframe: usize) -> usize {
  let diff = rate - (frameno as f32);
  if diff >= 1.0 {
    perframe
  } else {
    ((perframe as f32) * diff) as usize
  }
}

fn frame_duration(frameno: i32, rate: f32, second_start: PreciseTime) -> Duration {
  let ceil = rate.ceil() as i32;
  if frameno+1 < ceil {
    Duration::nanoseconds((1e9/rate) as i64)
  } else {
    // println!("final frame");
    Duration::seconds(1).sub(second_start.to(PreciseTime::now()))
  }
}

fn frame(fd:      &mut File,
         buf:     &mut [u8],
         szlimit: usize,
         tmlimit: Duration,
         fails:   &mut i32
        ) -> bool {
  let start = PreciseTime::now();
  let mut total:usize = 0;
  while total < szlimit {
    let remain = szlimit - total;
    let numread = read_at_most(fd, buf, remain);
    if start.to(PreciseTime::now()) > tmlimit {
      // println!("Out of time");
      *fails += 1;
      return false;
    }
    if numread == 0 {
      return true;
    }
    total += numread;
  }
  let delay = tmlimit.sub(start.to(PreciseTime::now())).num_milliseconds();
  // println!("Sleeping for {}ms", delay);
  if delay > 0 {
    sleep_ms(delay as u32);
  }
  false
}

fn next_frameno(current: i32, rate: f32) -> i32 {
  let ceil = rate.ceil() as i32;
  let nxt  = current + 1;
  nxt.rem(ceil)
}

fn read_at_most(fd: &mut File, buf: &mut[u8], count: usize) -> usize {
  if count < buf.len() {
    fd.read(&mut buf[..count]).unwrap()
  } else {
    fd.read(buf).unwrap()
  }
}

