
use std::env;
use std::io;
use std::process;

use spz_rs;

fn main() -> Result<(), io::Error> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Error: No filename provided. Usage {} FILENAME", args[0]);
        process::exit(-1);
    }

    let filename = &args[1];

    let packed_gaussians = spz_rs::load_packed_gaussians_from_file(filename)?;
    println!("File contains {} gaussians", packed_gaussians.num_points);

    if packed_gaussians.num_points > 0 {
        let unpacked_gaussian = packed_gaussians.unpack(0);
        println!("Splat 0 is at {}, {}, {}", 
            unpacked_gaussian.position[0],
            unpacked_gaussian.position[1],
            unpacked_gaussian.position[2]);
    }

    Ok(())
}