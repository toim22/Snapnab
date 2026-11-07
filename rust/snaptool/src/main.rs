//! SnapnabTool.exe: the helper modes the C++ SnapnabTests.exe had, for the e2e suite and for debugging.
//!
//!   SnapnabTool dump file.snap     prints the XML of the first frame's document
//!   SnapnabTool audio file.mp4     reads only the audio track: samples, end of stream and peak level

use std::io::Write;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match (args.first().map(String::as_str), args.get(1)) {
        (Some("dump"), Some(f)) => match snapcore::snap::load(Path::new(f)) {
            Some(frames) => {
                print!("{}", snapcore::xml::serialize(&frames[0].doc, true));
                0
            }
            None => {
                println!("failed to open");
                1
            }
        },
        (Some("audio"), Some(f)) => {
            print!("{}", snapcore::video::audio_report(f));
            0
        }
        _ => {
            eprintln!("usage: SnapnabTool dump <file.snap> | audio <file.mp4>");
            2
        }
    };
    let _ = std::io::stdout().flush();
    std::process::exit(code);
}
