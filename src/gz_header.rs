use std::{
    ffi::{CString, c_int, c_uint, c_ulong},
    mem::size_of,
};

use libz_sys::{deflateInit2_, gz_header, gz_headerp, z_stream, zlibVersion};

pub fn gz_header() -> Vec<u8> {
    let input = Input {
        source: String::from("Hello, world!"),
        config: DeflateConfig {
            level: libz_sys::Z_DEFAULT_COMPRESSION,
            method: libz_sys::Z_DEFLATED,
            window_bits: 16 + 15,
            mem_level: 8,
            strategy: libz_sys::Z_DEFAULT_STRATEGY,
        },
        chunk: 1024,
        flush: libz_sys::Z_FINISH,
        header: GzHeaderData {
            text: 2,
            time: 123,
            os: 3,
            extra: b"Hello, world!".to_vec(),
            name: c"hello_world.gz".to_owned(),
            comment: c"just some random comment".to_owned(),
            hcrc: 1234,
        },
    };

    compress_gz(input)
}

#[derive(Debug)]
struct GzHeaderData {
    text: i32,
    time: c_ulong,
    os: i32,
    extra: Vec<u8>,
    name: CString,
    comment: CString,
    hcrc: i32,
}

impl GzHeaderData {
    fn as_gz_header(&mut self) -> gz_header {
        gz_header {
            text: self.text,
            time: self.time,
            xflags: 0,
            os: self.os,
            extra: self.extra.as_mut_ptr(),
            extra_len: self.extra.len().try_into().unwrap(),
            extra_max: 0,                              // doesn't atter for writing.
            name: self.name.as_ptr() as *mut u8, // hack: UB if ritten to, but we shouldn't write during deflate.
            name_max: 0,                         // doesn't matter for writing.
            comment: self.comment.as_ptr() as *mut u8, // hack: UB if ritten to, but we shouldn't write during deflate.
            comm_max: 0,                               // doesn't atter for writing.
            hcrc: self.hcrc,
            done: 0, // doesn't matter for writing.
        }
    }
}

type Method = i32;
type Strategy = i32;

#[derive(Debug)]
pub struct DeflateConfig {
    pub level: i32,
    pub method: Method,
    pub window_bits: i32,
    pub mem_level: i32,
    pub strategy: Strategy,
}

#[derive(Debug)]
struct Input {
    source: String,
    config: DeflateConfig,
    chunk: u64,
    flush: c_int,
    header: GzHeaderData,
}

fn compress_gz(input: Input) -> Vec<u8> {
    let Input {
        mut source,
        config,
        chunk: _,
        flush,
        mut header,
    } = input;

    // Initialize stream.
    let mut stream = core::mem::MaybeUninit::zeroed();
    let streamp: *mut z_stream = stream.as_mut_ptr();
    let err = unsafe {
        deflateInit2_(
            streamp,
            config.level,
            config.method as i32,
            config.window_bits,
            config.mem_level,
            config.strategy as i32,
            zlibVersion(),
            size_of::<z_stream>() as c_int,
        )
    };
    if err != libz_sys::Z_OK as i32 {
        // Reject--the parameters are malformed.
        panic!()
    }

    // Create header.
    let mut header = header.as_gz_header();
    let err = unsafe { libz_sys::deflateSetHeader(streamp, &mut header as gz_headerp) };
    if err != libz_sys::Z_OK as i32 {
        dbg!(err);
        // Reject--we didn't request a gzip deflate stream.
        // Deallocate, so that we don't trigger ASAN leak detector.
        let err = unsafe { libz_sys::deflateEnd(streamp) };
        assert_eq!(err, libz_sys::Z_OK as i32);
        panic!();
    }

    let bound = unsafe { libz_sys::deflateBound(streamp, source.len() as u64) };
    let buf_size = bound * 2;

    let mut dest = vec![0; buf_size as usize];
    let max = c_uint::MAX as usize;

    let mut left = dest.len();
    let mut source_len = source.len();

    let stream = unsafe { stream.assume_init_mut() };

    stream.next_in = source.as_mut_ptr().cast();
    stream.next_out = dest.as_mut_ptr().cast();

    loop {
        if stream.avail_out == 0 {
            stream.avail_out = Ord::min(left, max) as _;
            left -= stream.avail_out as usize;
        }

        if stream.avail_in == 0 {
            stream.avail_in = Ord::min(source_len, max) as _;
            source_len -= stream.avail_in as usize;
        }

        let flush = if source_len > 0 {
            flush
        } else {
            libz_sys::Z_FINISH
        };

        let err = unsafe { libz_sys::deflate(streamp, flush as i32) };
        if err != libz_sys::Z_OK {
            break;
        }
    }

    dest.truncate(stream.total_out as usize);

    let err = unsafe { libz_sys::deflateEnd(streamp) };
    assert_eq!(err, libz_sys::Z_OK);

    dest
}
