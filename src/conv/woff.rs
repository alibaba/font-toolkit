//! A pure-Rust converter from WOFF to OTF for display.
//!
//! The `woff2otf` script was used as a reference: `https://github.com/hanikesn/woff2otf`
//!
//! See the WOFF spec: `http://people.mozilla.org/~jkew/woff/woff-spec-latest.html`
//!
//! This code is adopted from: https://github.com/pcwalton/rust-woff

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use flate2::read::ZlibDecoder;
use std::io::{self, Error, Read, Seek, SeekFrom, Write};
use std::mem;

/// "WOFF Header", http://people.mozilla.org/~jkew/woff/woff-spec-latest.html
#[allow(dead_code)]
struct WoffHeader {
    signature: u32,
    flavor: u32,
    length: u32,
    num_tables: u16,
    reserved: u16,
    total_sfnt_size: u32,
    major_version: u16,
    minor_version: u16,
    meta_offset: u32,
    meta_length: u32,
    meta_orig_length: u32,
    priv_offset: u32,
    priv_length: u32,
}

struct OtfHeader {
    flavor: u32,
    num_tables: u16,
    search_range: u16,
    entry_selector: u16,
    range_shift: u16,
}

/// "WOFF TableDirectoryEntry", http://people.mozilla.org/~jkew/woff/woff-spec-latest.html
struct WoffTableDirectoryEntry {
    tag: u32,
    offset: u32,
    comp_length: u32,
    orig_length: u32,
    orig_checksum: u32,
}

#[repr(C)]
struct OtfTableDirectoryEntry {
    tag: u32,
    orig_checksum: u32,
    offset: u32,
    orig_length: u32,
}

pub fn convert_woff_to_otf<R, W>(mut woff_reader: R, mut otf_writer: &mut W) -> Result<(), Error>
where
    R: Read + Seek,
    W: Write + Seek,
{
    // Hacker's Delight.
    fn previous_power_of_two(mut x: u16) -> u16 {
        x |= x >> 1;
        x |= x >> 2;
        x |= x >> 4;
        x |= x >> 8;
        x - (x >> 1)
    }

    fn tell<S>(stream: &mut S) -> Result<u64, Error>
    where
        S: Seek,
    {
        stream.seek(SeekFrom::Current(0))
    }

    // Read in headers.
    let woff_header = WoffHeader {
        signature: woff_reader.read_u32::<BigEndian>()?,
        flavor: woff_reader.read_u32::<BigEndian>()?,
        length: woff_reader.read_u32::<BigEndian>()?,
        num_tables: woff_reader.read_u16::<BigEndian>()?,
        reserved: woff_reader.read_u16::<BigEndian>()?,
        total_sfnt_size: woff_reader.read_u32::<BigEndian>()?,
        major_version: woff_reader.read_u16::<BigEndian>()?,
        minor_version: woff_reader.read_u16::<BigEndian>()?,
        meta_offset: woff_reader.read_u32::<BigEndian>()?,
        meta_length: woff_reader.read_u32::<BigEndian>()?,
        meta_orig_length: woff_reader.read_u32::<BigEndian>()?,
        priv_offset: woff_reader.read_u32::<BigEndian>()?,
        priv_length: woff_reader.read_u32::<BigEndian>()?,
    };

    let mut woff_table_directory_entries = Vec::with_capacity(woff_header.num_tables as usize);
    for _ in 0..woff_header.num_tables {
        woff_table_directory_entries.push(WoffTableDirectoryEntry {
            tag: woff_reader.read_u32::<BigEndian>()?,
            offset: woff_reader.read_u32::<BigEndian>()?,
            comp_length: woff_reader.read_u32::<BigEndian>()?,
            orig_length: woff_reader.read_u32::<BigEndian>()?,
            orig_checksum: woff_reader.read_u32::<BigEndian>()?,
        })
    }

    // Write out headers.
    let num_tables_previous_power_of_two = previous_power_of_two(woff_header.num_tables);
    let otf_search_range = num_tables_previous_power_of_two * 16;
    let otf_entry_selector = num_tables_previous_power_of_two.trailing_zeros() as u16;
    let otf_header = OtfHeader {
        flavor: woff_header.flavor,
        num_tables: woff_header.num_tables,
        search_range: otf_search_range,
        entry_selector: otf_entry_selector,
        range_shift: woff_header.num_tables * 16 - otf_search_range,
    };

    otf_writer
        .write_u32::<BigEndian>(otf_header.flavor)
        .unwrap();
    otf_writer
        .write_u16::<BigEndian>(otf_header.num_tables)
        .unwrap();
    otf_writer
        .write_u16::<BigEndian>(otf_header.search_range)
        .unwrap();
    otf_writer
        .write_u16::<BigEndian>(otf_header.entry_selector)
        .unwrap();
    otf_writer
        .write_u16::<BigEndian>(otf_header.range_shift)
        .unwrap();

    let mut otf_table_directory_entries = Vec::new();
    let mut otf_offset = tell(&mut otf_writer)? as u32
        + (mem::size_of::<OtfTableDirectoryEntry>() * woff_table_directory_entries.len()) as u32;
    for woff_table_directory_entry in &woff_table_directory_entries {
        let otf_table_directory_entry = OtfTableDirectoryEntry {
            tag: woff_table_directory_entry.tag,
            orig_checksum: woff_table_directory_entry.orig_checksum,
            offset: otf_offset,
            orig_length: woff_table_directory_entry.orig_length,
        };
        otf_writer
            .write_u32::<BigEndian>(otf_table_directory_entry.tag)
            .unwrap();
        otf_writer
            .write_u32::<BigEndian>(otf_table_directory_entry.orig_checksum)
            .unwrap();
        otf_writer
            .write_u32::<BigEndian>(otf_table_directory_entry.offset)
            .unwrap();
        otf_writer
            .write_u32::<BigEndian>(otf_table_directory_entry.orig_length)
            .unwrap();

        otf_offset += otf_table_directory_entry.orig_length;
        if otf_offset % 4 != 0 {
            otf_offset += 4 - otf_offset % 4
        }

        otf_table_directory_entries.push(otf_table_directory_entry);
    }
    // Decompress data if necessary, and write it out.
    for (woff_table_directory_entry, otf_table_directory_entry) in woff_table_directory_entries
        .iter()
        .zip(otf_table_directory_entries.iter())
    {
        debug_assert!(otf_table_directory_entry.offset as u64 == tell(&mut otf_writer)?);
        woff_reader.seek(SeekFrom::Start(woff_table_directory_entry.offset as u64))?;
        if woff_table_directory_entry.comp_length != woff_table_directory_entry.orig_length {
            let decoder = ZlibDecoder::new(woff_reader);
            let mut decoder = decoder.take(woff_table_directory_entry.orig_length as u64);
            io::copy(&mut decoder, &mut otf_writer)?;
            woff_reader = decoder.into_inner().into_inner();
        } else {
            let mut limited_woff_reader =
                (&mut woff_reader).take(woff_table_directory_entry.orig_length as u64);
            io::copy(&mut limited_woff_reader, &mut otf_writer)?;
        };
        woff_reader.seek(SeekFrom::Start(
            (woff_table_directory_entry.offset + woff_table_directory_entry.comp_length) as u64,
        ))?;

        let otf_end_offset =
            otf_table_directory_entry.offset + woff_table_directory_entry.orig_length;
        if otf_end_offset % 4 != 0 {
            let padding = 4 - otf_end_offset % 4;
            for _ in 0..padding {
                otf_writer.write_all(&[0])?
            }
        }
    }

    Ok(())
}

// #[cfg(test)]
// #[wasm_bindgen_test::wasm_bindgen_test]
// fn dump_woff() {
//     let result: &[u8] =
// include_bytes!("../../tests/HelveticaLTStd-Black.woff");     let read =
// std::io::Cursor::new(&result);     let mut out =
// std::io::Cursor::new(vec![]);     convert_woff_to_otf(read, &mut
// out).unwrap();     let out = out.into_inner();
//     let font = ttf_parser::Font::from_data(&out, 0).unwrap();
//     println!("{:?} {:?}", font.family_name(), font.post_script_name());
// }
