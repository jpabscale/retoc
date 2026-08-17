use std::io::Cursor;
use retoc::ser::ReadExt;
use retoc::name_map::FMappedName;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let data = std::fs::read(&path).unwrap();
    let mut c = Cursor::new(&data);
    let name: FMappedName = c.de().unwrap();
    let src: FMappedName = c.de().unwrap();
    let package_flags: u32 = c.de().unwrap();
    let cooked_header_size: u32 = c.de().unwrap();
    let nno: i32 = c.de().unwrap();
    let nns: i32 = c.de().unwrap();
    let nho: i32 = c.de().unwrap();
    let nhs: i32 = c.de().unwrap();
    let imo: i32 = c.de().unwrap();
    let emo: i32 = c.de().unwrap();
    let ebo: i32 = c.de().unwrap();
    let gdo: i32 = c.de().unwrap();
    let gds: i32 = c.de().unwrap();
    println!("name=({},{}) src=({},{}) flags=0x{:x} cooked={}", name.index(), name.number, src.index(), src.number, package_flags, cooked_header_size);
    println!("names_off={} names_size={} hashes_off={} hashes_size={}", nno, nns, nho, nhs);
    println!("import_off={} export_off={} bundle_off={} graph_off={} graph_size={}", imo, emo, ebo, gdo, gds);
}
