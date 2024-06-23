use std::collections::hash_map::DefaultHasher;
// use std::error::Error;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::PathBuf;

use clap::Parser;
use rand::RngCore;

fn main() {
    let opt = Opt::parse();
    if opt.generate.unwrap() {
        generate_file(&opt.name.into(), opt.length.unwrap_or(1024 * 1024));
        println!("\nFile Generated!")
    } else {
        check_file(&opt.name.into(), opt.length).unwrap_or_else(|e| {
            println!("{}", e);
            std::io::stdin().read_line(&mut String::new()).unwrap();
            panic!()
        });
        println!("\nFile Pass Check!");
    }
    // PAUSE
    std::io::stdin().read_line(&mut String::new()).unwrap();
}

static HASH_SIZE: usize = std::mem::size_of::<u64>();

/// Generate a file with the given name and length.
///
/// Method:
/// 1. The first sizeof(usize) bytes are filled with the length of the file.
/// 2. Then we generate a random val(u16) and write into the file.
/// 3. The following val bytes are filled with random data.
/// 4. Then we calculate the hash of the (sizeof(val) + val) bytes and write it into the file.
/// 5. After that we repeat step 2 to 4 until at least (length - sizeof(hash)) bytes are written.
/// 6. Finally we cut the file to (length - sizeof(hash)) bytes,
/// 7. and calculate the hash of it and write it into the file, sothat the file is exactly length bytes.
fn generate_file(name: &PathBuf, length: usize) {
    // step 0
    let mut data: Vec<u8> = Vec::new();
    // step 1
    data.extend_from_slice(&(length as u64).to_le_bytes());
    // step 5
    while data.len() < length - HASH_SIZE {
        // step 2
        let val = rand::random::<u16>();
        data.extend_from_slice(&val.to_le_bytes());
        // step 3
        let mut buf = vec![0; val as usize];
        rand::thread_rng().fill_bytes(&mut buf);
        data.extend_from_slice(&buf);
        // step 4
        let mut h = DefaultHasher::new();
        val.hash(&mut h);
        buf.hash(&mut h);
        let hash = h.finish();
        data.extend_from_slice(&hash.to_le_bytes());
        // print val and hash
        // print!("val: {}, hash: {}. ", val, hash);
        // print process
        print!(
            "\rGenerating: {}/{}                                     ",
            data.len(),
            length
        );
    }
    println!("\rGenerating: Done                                      ");
    // step 6
    data.truncate(length - HASH_SIZE);
    // step 7
    let mut h = DefaultHasher::new();
    data.hash(&mut h);
    let hash = h.finish();
    data.extend_from_slice(&hash.to_le_bytes());
    // check the length of data
    assert_eq!(data.len(), length);
    // write to file
    println!("Writing into file.");
    let mut file = File::create(name).unwrap();
    file.write_all(&data).unwrap();
}

/// Check the file with the given name and length.
/// If the file is not valid, return error.
fn check_file(name: &PathBuf, opt_length: Option<usize>) -> Result<(), String> {
    // check the length of data
    // or if length is not given, read from file.
    let data = std::fs::read(name).unwrap();
    let length = usize::from_le_bytes(data[..HASH_SIZE].try_into().unwrap());
    // assert!(opt_length.is_none() || opt_length.unwrap() == length);
    if let Some(len) = opt_length {
        if len != length {
            return Err(format!(
                "The length of the file is not correct. Expected [INPUT]: {} , Actual: {}",
                len, length
            ));
        }
    }
    // assert_eq!(data.len(), length);
    if data.len() != length {
        return Err(format!(
            "The length of the file is not correct. Expected [FILE_DEFINE]: {}, Actual: {}",
            length,
            data.len()
        ));
    }
    println!("Length of the file is correct: {}.", length);
    // step 7
    let hash = u64::from_le_bytes(data[length - HASH_SIZE..].try_into().unwrap());
    let mut h = DefaultHasher::new();
    (&data[..length - HASH_SIZE]).hash(&mut h);
    let h = h.finish();
    // assert_eq!(hash, h.finish());
    if hash != h {
        return Err(format!(
            "The hash of the file is not correct. Expected: {}, Actual: {}",
            hash, h
        ));
    }
    // step 1
    let mut data = &data[HASH_SIZE..length - HASH_SIZE];
    // step 5
    let mut block_index = 0;
    let mut block_offset = HASH_SIZE;
    // while data has at least 2 bytes
    while data.len() > 2 {
        // step 2
        let val = u16::from_le_bytes([data[0], data[1]]);
        data = &data[2..];
        if data.len() < val as usize + HASH_SIZE {
            // not complete hash to compare with. return.
            break;
        }
        // step 3
        let buf = &data[..val as usize];
        data = &data[val as usize..];
        // step 4
        let mut h = DefaultHasher::new();
        val.hash(&mut h);
        buf.hash(&mut h);
        let h = h.finish();
        let hash = u64::from_le_bytes(data[..HASH_SIZE].try_into().unwrap());
        // assert_eq!(hash, u64::from_le_bytes(hash1.try_into().unwrap()));
        if hash != h {
            return Err(format!(
                "The hash [Block<{}>, From<{}>, To<{}>] is not correct. Expected: {}, Actual: {}",
                block_index,
                block_offset,
                block_offset + 2 + val as usize,
                h,
                hash
            ));
        }
        // prepare for next loop
        data = &data[HASH_SIZE..];
        block_index += 1;
        block_offset += 2 + val as usize + HASH_SIZE;
        // print process
        print!("\rChecking block: {block_index}                      ");
    }
    print!("\rChecking: Done                                         ");
    return Ok(());
}

#[derive(Parser, Debug)]
#[clap(name = "file generator and checker.")]
struct Opt {
    /// The path to the file to be generated.
    name: String,

    /// The length of the file to be generated.
    /// Default is 1024 * 1000.
    #[arg(short, long)]
    length: Option<usize>,

    /// Generate the file.(Otherwise check the file.)
    /// If not provided, the file will be checked.
    #[arg(short, long, default_value = "false")]
    generate: Option<bool>,
}
