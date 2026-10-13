//! Differential-test oracle for `otio-time`.
//!
//! Reads tab-separated commands on stdin, one per line, and prints one result
//! line each. Used by `tests/diff_opentime.py` to compare against the reference
//! Python `opentimelineio.opentime`. Errors print the literal `ERR`.
//!
//! Commands:
//! - `to_timecode\t<value>\t<rate>`      -> timecode (InferFromRate) | ERR
//! - `from_timecode\t<tc>\t<rate>`       -> frame value (f64) | ERR
//! - `to_time_string\t<value>\t<rate>`   -> time string
//! - `from_time_string\t<s>\t<rate>`     -> seconds (f64) | ERR
//! - `to_seconds\t<value>\t<rate>`       -> seconds (f64)

use std::io::{self, BufRead, Write};

use otio_time::{IsDropFrameRate, RationalTime};

fn f(s: &str) -> f64 {
    s.parse::<f64>().expect("expected a float arg")
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.unwrap();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\t').collect();
        let result = match parts.as_slice() {
            ["to_timecode", value, rate] => {
                RationalTime::new(f(value), f(rate))
                    .to_timecode(f(rate), IsDropFrameRate::InferFromRate)
                    .unwrap_or_else(|_| "ERR".to_string())
            }
            ["from_timecode", tc, rate] => match RationalTime::from_timecode(tc, f(rate)) {
                Ok(rt) => format!("{}", rt.value()),
                Err(_) => "ERR".to_string(),
            },
            ["to_time_string", value, rate] => {
                RationalTime::new(f(value), f(rate)).to_time_string()
            }
            ["from_time_string", s, rate] => match RationalTime::from_time_string(s, f(rate)) {
                Ok(rt) => format!("{}", rt.to_seconds()),
                Err(_) => "ERR".to_string(),
            },
            ["to_seconds", value, rate] => {
                format!("{}", RationalTime::new(f(value), f(rate)).to_seconds())
            }
            _ => "ERR".to_string(),
        };
        writeln!(out, "{result}").unwrap();
    }
}
