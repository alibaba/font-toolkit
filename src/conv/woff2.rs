/// The following WOFF2 conversion logic mainly references code from [allsorts](https://github.com/yeslogic/allsorts)
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Cursor, Error, Read, Seek, Write};

const BITS_0_TO_5: u8 = 0x3F;
const LOWEST_UCODE: u16 = 253;
const BROTLI_DECODER_BUFFER_SIZE: usize = 4096;
const XFORM_TRANSFORM: u32 = 256;

const fn tag(chars: &[u8; 4]) -> u32 {
    (chars[3] as u32)
        | ((chars[2] as u32) << 8)
        | ((chars[1] as u32) << 16)
        | ((chars[0] as u32) << 24)
}

const GLYF: u32 = tag(b"glyf");
const HMTX: u32 = tag(b"hmtx");
const LOCA: u32 = tag(b"loca");

// When table tags are encoded into a WOFF2 TableDirectoryEntry this table is
// used to provide a one 5-bit encoding for common tables. The tables are in the
// order that they are encoded such that the value read from the file can be
// looked up in this array to get the corresponding tag. If the value is 0b11111
// (63), which is not present in this table, then this is an indication that a
// 4-byte tag follows the tag in the data stream.
// https://www.w3.org/TR/WOFF2/#table_dir_format
const KNOWN_TABLE_TAGS: [u32; 63] = [
    tag(b"cmap"),
    tag(b"head"),
    tag(b"hhea"),
    HMTX,
    tag(b"maxp"),
    tag(b"name"),
    tag(b"OS/2"),
    tag(b"post"),
    tag(b"cvt "),
    tag(b"fpgm"),
    GLYF,
    LOCA,
    tag(b"prep"),
    tag(b"cff "),
    tag(b"vorg"),
    tag(b"ebdt"),
    tag(b"eblc"),
    tag(b"gasp"),
    tag(b"hdmx"),
    tag(b"kern"),
    tag(b"ltsh"),
    tag(b"pclt"),
    tag(b"vdmx"),
    tag(b"vhea"),
    tag(b"vmtx"),
    tag(b"base"),
    tag(b"gdef"),
    tag(b"gpos"),
    tag(b"gsub"),
    tag(b"ebsc"),
    tag(b"jstf"),
    tag(b"math"),
    tag(b"cbdt"),
    tag(b"cblc"),
    tag(b"colr"),
    tag(b"cpal"),
    tag(b"svg "),
    tag(b"sbix"),
    tag(b"acnt"),
    tag(b"avar"),
    tag(b"bdat"),
    tag(b"bloc"),
    tag(b"bsln"),
    tag(b"cvar"),
    tag(b"fdsc"),
    tag(b"feat"),
    tag(b"fmtx"),
    tag(b"fvar"),
    tag(b"gvar"),
    tag(b"hsty"),
    tag(b"just"),
    tag(b"lcar"),
    tag(b"mort"),
    tag(b"morx"),
    tag(b"opbd"),
    tag(b"prop"),
    tag(b"trak"),
    tag(b"zapf"),
    tag(b"silf"),
    tag(b"glat"),
    tag(b"gloc"),
    tag(b"Feat"),
    tag(b"sill"),
];

// Hacker's Delight.
const fn previous_power_of_two(mut x: u16) -> u16 {
    x |= x >> 1;
    x |= x >> 2;
    x |= x >> 4;
    x |= x >> 8;
    x - (x >> 1)
}

fn read_u32_base_128<R>(reader: &mut R) -> Result<u32, Error>
where
    R: Read + Seek,
{
    let mut accum = 0u32;

    for i in 0..5 {
        let byte = reader.read_u8()?;

        // No leading 0's
        if i == 0 && byte == 0x80 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }

        // If any of the top 7 bits are set then << 7 would overflow
        if accum & 0xFE000000 != 0 {
            return Err(std::io::ErrorKind::InvalidData.into());
        }

        // value = old value times 128 + (byte bitwise-and 127)
        accum = (accum << 7) | u32::from(byte & 0x7F);

        // Spin until most significant bit of data byte is false
        if byte & 0x80 == 0 {
            return Ok(accum);
        }
    }

    // UIntBase128 sequence exceeds 5 bytes
    Err(std::io::ErrorKind::InvalidData.into())
}

fn read_packed_u16<R>(reader: &mut R) -> Result<u16, Error>
where
    R: Read + Seek,
{
    match reader.read_u8()? {
        253 => reader.read_u16::<BigEndian>(),
        254 => reader
            .read_u8()
            .map(|value| u16::from(value) + LOWEST_UCODE * 2),
        255 => reader
            .read_u8()
            .map(|value| u16::from(value) + LOWEST_UCODE),
        code => Ok(u16::from(code)),
    }
}

struct TableDirectoryEntry {
    pub tag: u32,
    pub offset: usize,
    pub orig_length: u32,
    pub transform_length: u32,
}

pub fn convert_woff2_to_otf<T, W>(
    mut woff_reader: Cursor<T>,
    mut otf_writer: W,
) -> Result<(), Error>
where
    W: Write + Seek,
    T: AsRef<[u8]>,
{
    let _ = woff_reader.read_u32::<BigEndian>()?; // Signature
    let flavor = woff_reader.read_u32::<BigEndian>()?;
    let _length = woff_reader.read_u32::<BigEndian>()?;
    let num_tables = woff_reader.read_u16::<BigEndian>()?;
    let _ = woff_reader.read_u16::<BigEndian>()?; // Reserved, should be zero
    let _total_sfnt_size = woff_reader.read_u32::<BigEndian>()?;
    let total_compressed_size = woff_reader.read_u32::<BigEndian>()?;
    let _ = woff_reader.read_u16::<BigEndian>()?; // Major version
    let _ = woff_reader.read_u16::<BigEndian>()?; // Minor version
    let _meta_offset = woff_reader.read_u32::<BigEndian>()?;
    let _meta_length = woff_reader.read_u32::<BigEndian>()?;
    let _meta_orig_length = woff_reader.read_u32::<BigEndian>()?;
    let _priv_offset = woff_reader.read_u32::<BigEndian>()?;
    let _priv_length = woff_reader.read_u32::<BigEndian>()?;
    let num_tables_previous_power_of_two = previous_power_of_two(num_tables);
    let otf_search_range = num_tables_previous_power_of_two * 16;
    let otf_entry_selector = num_tables_previous_power_of_two.trailing_zeros() as u16;
    otf_writer.write_u32::<BigEndian>(flavor)?;
    otf_writer.write_u16::<BigEndian>(num_tables)?;
    otf_writer.write_u16::<BigEndian>(otf_search_range)?;
    otf_writer.write_u16::<BigEndian>(otf_entry_selector)?;
    otf_writer.write_u16::<BigEndian>(num_tables * 16 - otf_search_range)?;

    // Read table directory entries
    let mut table_dir_entries = Vec::with_capacity(num_tables as usize);
    let mut offset = 0; //tell(&mut otf_writer)? as usize;
    for _ in 0..num_tables {
        let flags = woff_reader.read_u8()?;
        let tag = if flags & BITS_0_TO_5 == BITS_0_TO_5 {
            // Tag is the following 4 bytes
            woff_reader.read_u32::<BigEndian>()
        } else {
            Ok(KNOWN_TABLE_TAGS[usize::from(flags & BITS_0_TO_5)])
        }?;
        let xform_version = (flags >> 6) & 0x03;

        let mut flags = 0;
        if tag == GLYF || tag == LOCA {
            if xform_version == 0 {
                flags |= XFORM_TRANSFORM;
            }
        } else if xform_version != 0 {
            flags |= XFORM_TRANSFORM;
        };
        flags |= xform_version as u32;

        let orig_length = read_u32_base_128(&mut woff_reader)?;
        let mut transform_length = orig_length;
        if (flags & XFORM_TRANSFORM) != 0 {
            transform_length = read_u32_base_128(&mut woff_reader)?;
        }

        let entry = TableDirectoryEntry {
            tag,
            offset,
            orig_length,
            transform_length,
        };

        offset += entry.transform_length as usize;
        table_dir_entries.push(entry);
    }

    // Font collection, we only use the first font
    if flavor == 0x74746366 {
        let _ = woff_reader.read_u32::<BigEndian>()?; // TTC version
        let num_fonts = read_packed_u16(&mut woff_reader)?;
        for _ in 0..num_fonts {
            let num_tables = read_packed_u16(&mut woff_reader)?;
            let _ = woff_reader.read_u32::<BigEndian>()?; // flavor
            for _ in 0..num_tables {
                let _ = read_packed_u16(&mut woff_reader)?;
            }
        }
    }

    // Read table data
    let offset = woff_reader.position() as usize;
    let buffer = woff_reader.into_inner();
    let compressed = &buffer.as_ref()[offset..(offset + total_compressed_size as usize)];
    let mut buffer = brotli_decompressor::Decompressor::new(compressed, BROTLI_DECODER_BUFFER_SIZE);

    let mut table_data_block = Vec::new();
    buffer.read_to_end(&mut table_data_block)?;

    let data_offset = 12 + table_dir_entries.len() * 16;
    for entry in &table_dir_entries {
        otf_writer.write_u32::<BigEndian>(entry.tag)?;
        otf_writer.write_u32::<BigEndian>(0)?; // checksum
        otf_writer.write_u32::<BigEndian>((entry.offset + data_offset) as u32)?;
        otf_writer.write_u32::<BigEndian>(entry.orig_length)?;
    }
    otf_writer.write_all(&table_data_block)?;
    Ok(())
}

// #[cfg(test)]
// mod tests {
//     use super::*;
//     use fontkit::LoadedFont;
//     use std::fs::File;
//     use std::io::{Cursor, Read};
//     use test_helpers::correct_file;

//     #[test]
//     fn test_woff2_convertor() {
//         let mut f = File::open(correct_file!(
//             "../../fixtures/fonts/Alibaba-PuHuiTi-Bold.woff2"
//         ))
//         .unwrap();
//         let mut buffer = vec![];
//         f.read_to_end(&mut buffer).unwrap();
//         let mut otf_buffer = Cursor::new(vec![]);
//         let buffer = Cursor::new(buffer);
//         convert_woff2_to_otf(buffer, &mut otf_buffer).unwrap();
//         let otf_buffer = otf_buffer.into_inner();
//         let font = LoadedFont::from_face(otf_buffer).unwrap();
//         println!("{:?}", font.measure('测', Some(&font), false).unwrap());
//         assert_eq!(font.font().weight, 700);
//     }
// }
