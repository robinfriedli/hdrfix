use std::fs::File;
use std::io;
use std::io::{Error, ErrorKind};

// Math bits
use glam::f32::{Mat3, Vec3};

// CLI bits
extern crate clap;
use clap::{Arg, App, ArgMatches};

// Reading and writing files
extern crate png;
extern crate mtpng;
use mtpng::{CompressionLevel, Header};
use mtpng::encoder::{Encoder, Options};
use mtpng::ColorType;

pub fn err(payload: &str) -> Error
{
    io::Error::new(ErrorKind::Other, payload)
}

// Read an input PNG and return its size and contents
// It must be a certain format (8bpp true color no alpha)
fn read_png(filename: &str)
    -> io::Result<(u32, u32, Vec<u8>)>
{
    use png::Decoder;
    use png::Transformations;

    let mut decoder = Decoder::new(File::open(filename)?);
    decoder.set_transformations(Transformations::IDENTITY);

    let (info, mut reader) = decoder.read_info()?;

    if info.bit_depth != png::BitDepth::Eight {
        return Err(err("color depth must be 8 bpp currently"));
    }
    if info.color_type != png::ColorType::RGB {
        return Err(err("color type must be true color with no alpha"));
    }

    let mut data = vec![0u8; info.buffer_size()];
    reader.next_frame(&mut data)?;

    Ok((info.width, info.height, data))
}

fn pq_to_linear(val: Vec3) -> Vec3 {
    // fixme make sure all the splats are efficient constants
    let inv_m1: f32 = 1.0 / 0.1593017578125;
    let inv_m2: f32 = 1.0 / 78.84375;
    let c1 = Vec3::splat(0.8359375);
    let c2 = Vec3::splat(18.8515625);
    let c3 = Vec3::splat(18.6875);
    let val_powered = val.powf(inv_m2);
    (Vec3::max(val_powered - c1, Vec3::ZERO)
        / (c2 - c3 * val_powered)
    ).powf(inv_m1)
}

fn bt2020_to_srgb(val: Vec3) -> Vec3 {
    let matrix = Mat3::from_cols_array(&[
        1.6605, -0.1246, -0.0182,
        -0.5876, 1.1329, -0.1006,
        -0.0728, -0.0083, 1.1187
    ]);
    matrix.mul_vec3(val)
}

fn reinhold_tonemap(val: Vec3) -> Vec3 {
    // TMO_reinhardext​(C) = C(1 + C/C_white^2​) / (1 + C)
    let white = 1000.0; // fake a nominal max of 1000 nits in scene
    let white2 = Vec3::splat(white * white);
    val * (Vec3::ONE + val / white2) / (Vec3::ONE + val)
}

fn linear_to_srgb(val: Vec3) -> Vec3 {
    // fixme make sure all the splats are efficient constants
    let min = Vec3::splat(0.0031308);
    let linear = val * Vec3::splat(12.92);
    let gamma = (val * Vec3::splat(1.055)).powf(1.0 / 2.4) - Vec3::splat(0.055);
    Vec3::select(val.cmple(min), linear, gamma)
}

fn hdr_to_sdr(width: u32, height: u32, data: &mut [u8])
{
    // 80 nits is the nominal SDR white point
    // but daytime monitors likely set more like 200
    let sdr_white = 200.0;
    for y in 0..height {
        let scale_in = Vec3::splat(1.0 / 255.0);
        let scale_hdr = Vec3::splat(10000.0 / sdr_white);
        let scale_out = Vec3::splat(255.0);
        for x in 0..width {
            // Read the original pixel value
            let index = ((x + y * width) * 3) as usize;
            let r1 = data[index] as f32;
            let g1 = data[index + 1] as f32;
            let b1 = data[index + 2] as f32;
            let mut val = Vec3::new(r1, g1, b1);
            val = val * scale_in;
            val = pq_to_linear(val);
            val = val * scale_hdr;
            val = bt2020_to_srgb(val);
            val = reinhold_tonemap(val);
            val = linear_to_srgb(val);
            val = val * scale_out;
            data[index] = val.x as u8;
            data[index + 1] = val.y as u8;
            data[index + 2] = val.z as u8;
        }
    }
}

fn write_png(filename: &str,
             width: u32,
             height: u32,
             data: &[u8])
   -> io::Result<()>
{
    let writer = File::create(filename)?;

    let mut options = Options::new();
    options.set_compression_level(CompressionLevel::High)?;

    let mut header = Header::new();
    header.set_size(width, height)?;
    header.set_color(ColorType::Truecolor, 8)?;

    let mut encoder = Encoder::new(writer, &options);

    encoder.write_header(&header)?;
    encoder.write_image_rows(&data)?;
    encoder.finish()?;

    Ok(())
}

fn hdrfix(args: ArgMatches) -> io::Result<String> {
    let input_filename = args.value_of("input").unwrap();
    let (width, height, mut data) = read_png(input_filename)?;

    hdr_to_sdr(width, height, &mut data);

    let output_filename = args.value_of("output").unwrap();
    write_png(output_filename, width, height, &data)?;

    return Ok(output_filename.to_string());
}

fn main() {
    let args = App::new("hdrfix converter for HDR screenshots")
        .version("0.1.0")
        .author("Brion Vibber <brion@pobox.com>")
        .arg(Arg::with_name("input")
            .help("Input filename, must be .png as saved by Nvidia capture overlay")
            .required(true)
            .index(1))
        .arg(Arg::with_name("output")
            .help("Output filename, must be .png.")
            .required(true)
            .index(2))
        .get_matches();

    match hdrfix(args) {
        Ok(outfile) => println!("Saved: {}", outfile),
        Err(e) => eprintln!("Error: {}", e),
    }
}
