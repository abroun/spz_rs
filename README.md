# spz_rs

This crate contains Rust code for reading in Gaussian Splats stored in the Niantic .spz file format.

Currently this crate only supports reading .spz files. Support for writing .spz files is planned and hopefully coming soon.

This crate was created by translating the code from the reference Niantic C++ implementation which can be found at
https://github.com/nianticlabs/spz. The implementation of this crate is in pure Rust and makes no use of the C++ code
at runtime, but we still reference the C++ repo as a submodule so that we can access the sample files in our example code
and to provide a baseline for our planned benchmark code.

## Usage

```rust
use spz_rs;

let packed_gaussians = spz_rs::load_packed_gaussians_from_file(filename)?;
println!("File contains {} gaussians", packed_gaussians.num_points);

if packed_gaussians.num_points > 0 {
    let unpacked_gaussian = packed_gaussians.unpack(0);
    println!("Splat 0 is at {}, {}, {}", 
        unpacked_gaussian.position[0],
        unpacked_gaussian.position[1],
        unpacked_gaussian.position[2]);
}
```

## Credits

This crate was started thanks to a bit of work sponsored by [Waldek Technologies](https://www.gauzilla.xyz/), makers of  AI-Powered 3D Gaussian Splatting tools.