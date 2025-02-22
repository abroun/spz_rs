// Translated from the Niantic C++ reference implementation at https://github.com/nianticlabs/spz
// Provides support for loading in Gaussian splats stored in the Niantic .spz format

//  Copyright (c) 2025 Alan Broun, Beholder Vision Ltd

use std::fs;
use std::io;
use std::mem;

use flate2::read::GzDecoder;

const FLAG_ANTIALIASED: u8 = 0x1;

// Scale factor for DC color components. To convert to RGB, we should multiply by 0.282, but it can
// be useful to represent base colors that are out of range if the higher spherical harmonics bands
// bring them back into range so we multiply by a smaller value.
const COLOR_SCALE: f32 = 0.15;

fn dim_for_degree(degree: usize) -> usize {
    match degree {
        0 => 0,
        1 => 3,
        2 => 8,
        3 => 15,
        _ => {
            eprintln!("[SPZ: ERROR] Unsupported SH degree: {}", degree);
            0
        }
    }
}

fn half_to_f32(h: u16) -> f32 {
    let sgn = (h >> 15) & 0x1;
    let exponent = (h >> 10) & 0x1f;
    let mantissa = h & 0x3ff;

    let sign_mul = if sgn == 1 { -1.0 } else { 1.0 };
    if exponent == 0 {
        // Subnormal numbers (no exponent, 0 in the mantissa decimal).
        return sign_mul * 2.0f32.powf(-14.0) * (mantissa as f32) / 1024.0;
    }

    if exponent == 31 {
        // Infinity or NaN.
        if mantissa == 0 { 
            return sign_mul * f32::INFINITY;
        } else {
            return f32::NAN;
        }
    }

    // non-zero exponent implies 1 in the mantissa decimal.
    return sign_mul * 2.0f32.powf((exponent as f32) - 15.0)
        * (1.0 + (mantissa as f32) / 1024.0);
}

fn unquantize_scale(x: u8) -> f32 {
    x as f32 / 16.0 - 10.0
}

fn unquantize_alpha(x: u8) -> f32 {
    inv_sigmoid(x as f32 / 255.0)
}

fn unquantize_sh(x: u8) -> f32 {
    ((x as f32) - 128.0) / 128.0
}

fn inv_sigmoid(x: f32) -> f32 { 
    (x / (1.0 - x)).ln()
}

#[derive(Default)]
pub struct UnpackedGaussian {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub color: [f32; 3],
    pub alpha: f32,
    pub sh_r: [f32; 15],
    pub sh_g: [f32; 15],
    pub sh_b: [f32; 15],
}

#[derive(Default)]
pub struct PackedGaussian {
    pub position: [u8; 9],
    pub rotation: [u8; 3],
    pub scale: [u8; 3],
    pub color: [u8; 3],
    pub alpha: u8,
    pub sh_r: [u8; 15],
    pub sh_g: [u8; 15],
    pub sh_b: [u8; 15],
}

impl PackedGaussian {
    pub fn unpack(&self, uses_float16: bool, fractional_bits: u32) -> UnpackedGaussian {
        let mut result = UnpackedGaussian::default();

        if uses_float16 {
            for i in 0..3 {
                result.position[i] = half_to_f32(self.position[i * 2] as u16);
            }
        } else {
            let scale = 1.0 / (1 << fractional_bits) as f32;
            for i in 0..3 {
                let mut fixed32: i32 = self.position[i * 3] as i32;
                fixed32 |= (self.position[i * 3 + 1] as i32) << 8;
                fixed32 |= (self.position[i * 3 + 2] as i32) << 16;
                fixed32 |= if fixed32 & 0x800000 != 0 { 0xff000000u32 as i32 } else { 0 };
                result.position[i] = fixed32 as f32 * scale;
            }
        }

        for i in 0..3 {
            result.scale[i] = unquantize_scale(self.scale[i]);
        }

        // Decode quaternion and store as w, x, y, z
        let xyz = self.rotation.iter().map(|&r| r as f32 / 127.5 - 1.0).collect::<Vec<f32>>();
        result.rotation[1..].clone_from_slice(&xyz);
        result.rotation[0] = (1.0 - xyz.iter().map(|v| v * v).sum::<f32>()).sqrt().max(0.0);

        result.alpha = unquantize_alpha(self.alpha);

        for i in 0..3 {
            result.color[i] = (self.color[i] as f32 / 255.0 - 0.5) / COLOR_SCALE;
        }

        for i in 0..15 {
            result.sh_r[i] = unquantize_sh(self.sh_r[i]);
            result.sh_g[i] = unquantize_sh(self.sh_g[i]);
            result.sh_b[i] = unquantize_sh(self.sh_b[i]);
        }

        result
    }
}

#[repr(C)]
pub struct PackedGaussiansHeader {
    pub magic: u32,
    pub version: u32,
    pub num_points: u32,
    pub sh_degree: u8,
    pub fractional_bits: u8,
    pub flags: u8,
    pub reserved: u8,
}

impl Default for PackedGaussiansHeader {
    fn default() -> PackedGaussiansHeader {
        PackedGaussiansHeader {
            magic: 0x5053474e,  // NGSP = Niantic gaussian splat
            version: 2,
            num_points: 0,
            sh_degree: 0,
            fractional_bits: 0,
            flags: 0,
            reserved: 0,
        }
    }
}

pub struct PackedGaussians {
    pub num_points: usize,
    pub sh_degree: usize,
    pub fractional_bits: usize,
    pub antialiased: bool,
    pub positions: Vec<u8>,
    pub scales: Vec<u8>,
    pub rotations: Vec<u8>,
    pub alphas: Vec<u8>,
    pub colors: Vec<u8>,
    pub sh: Vec<u8>,
}

impl PackedGaussians {
    pub fn uses_float16(&self) -> bool {
        self.positions.len() == self.num_points * 3 * 2
    }

    pub fn at(&self, i: usize) -> PackedGaussian {
        let mut result = PackedGaussian::default();
        let position_bits = if self.uses_float16() { 6 } else { 9 };

        let start3 = i * 3;
        let p_start = i * position_bits;
        result.position.copy_from_slice(&self.positions[p_start..p_start + position_bits]);
        result.scale.copy_from_slice(&self.scales[start3..start3 + 3]);
        result.rotation.copy_from_slice(&self.rotations[start3..start3 + 3]);
        result.color.copy_from_slice(&self.colors[start3..start3 + 3]);
        result.alpha = self.alphas[i];

        let sh_dim = dim_for_degree(self.sh_degree);
        let sh_start = i * sh_dim * 3;
        for j in 0..sh_dim {
            result.sh_r[j] = self.sh[sh_start + j * 3];
            result.sh_g[j] = self.sh[sh_start + j * 3 + 1];
            result.sh_b[j] = self.sh[sh_start + j * 3 + 2];
        }
        for j in sh_dim..15 {
            result.sh_r[j] = 128;
            result.sh_g[j] = 128;
            result.sh_b[j] = 128;
        }

        result
    }

    pub fn unpack(&self, i: usize) -> UnpackedGaussian {
        self.at(i).unpack(self.uses_float16(), self.fractional_bits as u32)
    }

    pub fn unpack_scale(&self, i: usize) -> [f32; 3] {
        [unquantize_scale(self.scales[3*i]), unquantize_scale(self.scales[3*i + 1]), unquantize_scale(self.scales[3*i + 2])]
    }

    pub fn unpack_alpha(&self, i: usize) -> f32 {
        unquantize_alpha(self.alphas[i])
    }
}

pub fn load_packed_gaussians_from_decompressed_buffer<R: io::Read>(mut reader: R) -> Result<PackedGaussians, std::io::Error> {
    let header: PackedGaussiansHeader = {   // From https://users.rust-lang.org/t/read-into-struct/30972/4
        let mut h = [0u8; size_of::<PackedGaussiansHeader>()];
        reader.read_exact(&mut h[..])?;
        unsafe { mem::transmute(h) }
    };

    if header.magic != PackedGaussiansHeader::default().magic {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Header not found"));
    }

    if header.version < 1 || header.version > 2 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Unsupported version"));
    }

    let num_points = header.num_points as usize;
    let sh_dim = dim_for_degree(header.sh_degree as usize);
    let uses_float16 = header.version == 1;

    let mut result = PackedGaussians {
        num_points,
        sh_degree: header.sh_degree as usize,
        fractional_bits: header.fractional_bits as usize,
        antialiased: (header.flags & FLAG_ANTIALIASED) != 0,
        positions: vec![0; num_points * 3 * if uses_float16 { 2 } else { 3 }],
        scales: vec![0; num_points * 3],
        rotations: vec![0; num_points * 3],
        alphas: vec![0; num_points],
        colors: vec![0; num_points * 3],
        sh: vec![0; num_points * sh_dim * 3],
    };

    reader.read_exact(&mut result.positions)?;
    reader.read_exact(&mut result.alphas)?;
    reader.read_exact(&mut result.colors)?;
    reader.read_exact(&mut result.scales)?;
    reader.read_exact(&mut result.rotations)?;
    reader.read_exact(&mut result.sh)?;

    Ok(result)
}

pub fn load_packed_gaussians_from_spz_buffer<R: io::Read>(reader: R) -> Result<PackedGaussians, std::io::Error> {

    let gz_decoder = GzDecoder::new(reader);
    load_packed_gaussians_from_decompressed_buffer(gz_decoder)
}

pub fn load_packed_gaussians_from_file(filename: &String) -> Result<PackedGaussians, std::io::Error> {

    let file = fs::File::open(filename)?;
    let reader = io::BufReader::new(file);
    load_packed_gaussians_from_spz_buffer(reader)
}