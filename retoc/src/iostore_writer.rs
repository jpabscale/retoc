use crate::{
    EIoChunkType, FPackageId, UEPath, UEPathBuf, align_usize,
    chunk_id::FIoChunkIdRaw,
    container_header::{EIoContainerHeaderVersion, FIoContainerHeader, StoreEntry},
};
use crate::{EIoStoreTocVersion, FIoChunkHash, FIoChunkId, FIoContainerId, FIoOffsetAndLength, FIoStoreTocCompressedBlockEntry, FIoStoreTocEntryMeta, FIoStoreTocEntryMetaFlags, Toc, ser::*};
use anyhow::{Context, Result};
use fs_err as fs;
use std::io::Cursor;
use std::{
    io::{BufWriter, Seek, Write},
    path::{Path, PathBuf},
};

pub struct IoStoreWriter {
    #[allow(unused)]
    toc_path: PathBuf,
    toc_stream: BufWriter<fs::File>,
    cas_stream: BufWriter<fs::File>,
    toc: Toc,
    container_header: Option<FIoContainerHeader>,
}

impl IoStoreWriter {
    pub fn new<P: AsRef<Path>>(toc_path: P, toc_version: EIoStoreTocVersion, container_header_version: Option<EIoContainerHeaderVersion>, mount_point: UEPathBuf) -> Result<Self> {
        Self::with_container_id(toc_path, toc_version, container_header_version, mount_point, None)
    }
    pub fn with_container_id<P: AsRef<Path>>(toc_path: P, toc_version: EIoStoreTocVersion, container_header_version: Option<EIoContainerHeaderVersion>, mount_point: UEPathBuf, container_id: Option<FIoContainerId>) -> Result<Self> {
        let toc_path = toc_path.as_ref().to_path_buf();
        let name = toc_path.file_stem().unwrap().to_string_lossy();
        let toc_stream = BufWriter::new(fs::File::create(&toc_path)?);
        let cas_stream = BufWriter::new(fs::File::create(toc_path.with_extension("ucas"))?);

        let mut toc = Toc::new();
        toc.compression_block_size = 0x10000;
        toc.version = toc_version;
        toc.container_id = container_id.unwrap_or_else(|| FIoContainerId::from_name(&name));
        toc.directory_index.mount_point = mount_point;
        toc.partition_size = u64::MAX;

        let container_header = container_header_version.map(|v| FIoContainerHeader::new(v, toc.container_id));

        Ok(Self {
            toc_path,
            toc_stream,
            cas_stream,
            toc,
            container_header,
        })
    }
    pub fn with_container_header<P: AsRef<Path>>(toc_path: P, toc_version: EIoStoreTocVersion, mount_point: UEPathBuf, container_header: Option<FIoContainerHeader>) -> Result<Self> {
        let toc_path = toc_path.as_ref().to_path_buf();
        let name = toc_path.file_stem().unwrap().to_string_lossy();
        let toc_stream = BufWriter::new(fs::File::create(&toc_path)?);
        let cas_stream = BufWriter::new(fs::File::create(toc_path.with_extension("ucas"))?);

        let mut toc = Toc::new();
        toc.compression_block_size = 0x10000;
        toc.version = toc_version;
        toc.container_id = container_header.as_ref().map(|header| header.container_id).unwrap_or_else(|| FIoContainerId::from_name(&name));
        toc.directory_index.mount_point = mount_point;
        toc.partition_size = u64::MAX;

        Ok(Self { toc_path, toc_stream, cas_stream, toc, container_header })
    }
    pub fn write_chunk_raw(&mut self, chunk_id_raw: FIoChunkIdRaw, path: Option<&UEPath>, data: &[u8]) -> Result<()> {
        self.write_chunk(FIoChunkId::from_raw(chunk_id_raw, self.toc.version), path, data)
    }
    /// Writes a chunk, and if [store_entry] is provided and the chunk is a package export bundle,
    /// registers it in the container header so the game can resolve the package. Use for raw
    /// repacks where the package metadata was carried over from the source container.
    pub fn write_chunk_auto(&mut self, chunk_id_raw: FIoChunkIdRaw, path: Option<&UEPath>, data: &[u8], store_entry: Option<StoreEntry>) -> Result<()> {
        let chunk_id = FIoChunkId::from_raw(chunk_id_raw, self.toc.version);
        if let Some(store_entry) = store_entry {
            if let Some(header) = self.container_header.as_mut() {
                header.add_package(FPackageId(chunk_id.get_chunk_id()), store_entry);
            }
        }
        self.write_chunk(chunk_id, path, data)
    }
    pub fn write_chunk(&mut self, chunk_id: FIoChunkId, path: Option<&UEPath>, data: &[u8]) -> Result<()> {
        if let Some(path) = path {
            let index = &mut self.toc.directory_index;
            let relative_path = path.strip_prefix(&index.mount_point).with_context(|| format!("mount point {} does not contain path {path}", index.mount_point))?;
            index.add_file(relative_path, self.toc.chunks.len() as u32);
        }

        let mut offset = self.cas_stream.stream_position()?;

        let start_block = self.toc.compression_blocks.len();

        let mut hasher = blake3::Hasher::new();
        for block in data.chunks(self.toc.compression_block_size as usize) {
            self.cas_stream.write_all(block)?;
            hasher.update(block);
            let compressed_size = block.len() as u32;
            let uncompressed_size = block.len() as u32;
            let compression_method_index = 0; // "None"
            self.toc.compression_blocks.push(FIoStoreTocCompressedBlockEntry::new(offset, compressed_size, uncompressed_size, compression_method_index));
            offset += compressed_size as u64;
        }
        let hash = hasher.finalize();
        let meta = FIoStoreTocEntryMeta {
            chunk_hash: FIoChunkHash::from_blake3(hash.as_bytes()),
            flags: FIoStoreTocEntryMetaFlags::empty(),
        };

        let offset_and_length = FIoOffsetAndLength::new(start_block as u64 * self.toc.compression_block_size as u64, data.len() as u64);

        self.toc.chunks.push(chunk_id.with_version(self.toc.version));
        self.toc.chunk_offset_lengths.push(offset_and_length);
        self.toc.chunk_metas.push(meta);

        Ok(())
    }

    pub fn write_package_chunk(&mut self, chunk_id: FIoChunkId, path: Option<&UEPath>, data: &[u8], store_entry: &StoreEntry) -> Result<()> {
        let container_header = self.container_header.as_mut().expect("FIoContainerHeader is required to write package chunks");
        container_header.add_package(FPackageId(chunk_id.get_chunk_id()), store_entry.clone());
        self.write_chunk(chunk_id, path, data)
    }
    pub fn add_localized_package(&mut self, package_culture: &str, source_package_name: &str, localized_package_id: FPackageId) -> Result<()> {
        let container_header = self.container_header.as_mut().expect("FIoContainerHeader is required to add localized packages");
        container_header.add_localized_package(package_culture, source_package_name, localized_package_id)
    }
    pub fn add_package_redirect(&mut self, source_package_name: &str, redirect_package_id: FPackageId) -> Result<()> {
        let container_header = self.container_header.as_mut().expect("FIoContainerHeader is required to add package redirects");
        container_header.add_package_redirect(source_package_name, redirect_package_id)
    }
    pub fn container_version(&self) -> EIoStoreTocVersion {
        self.toc.version
    }
    pub fn container_header_version(&self) -> EIoContainerHeaderVersion {
        self.container_header.as_ref().unwrap().version
    }
    pub fn finalize(mut self) -> Result<()> {
        if let Some(container_header) = &self.container_header {
            let mut chunk_buffer = vec![];
            container_header.serialize(&mut Cursor::new(&mut chunk_buffer))?;
            // container header is always aligned for AES for some reason
            chunk_buffer.resize(align_usize(chunk_buffer.len(), 16), 0);

            let chunk_id = FIoChunkId::create(container_header.container_id.0, 0, EIoChunkType::ContainerHeader);
            self.write_chunk(chunk_id, None, &chunk_buffer)?;
        }
        self.toc_stream.ser(&self.toc)?;
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::Config;
    use fs_err as fs;
    use crate::iostore::IoStoreTrait;
    use std::sync::Arc;

    #[test]
    fn test_write_container() -> Result<()> {
        fs::create_dir("out").ok();
        let mut writer = IoStoreWriter::new("out/new.utoc", EIoStoreTocVersion::PerfectHashWithOverflow, Some(EIoContainerHeaderVersion::OptionalSegmentPackages), "../../..".into())?;

        let data = fs::read("tests/UE5.3/ScriptObjects.bin")?;
        writer.write_chunk_raw(FIoChunkIdRaw { id: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 5] }, Some(UEPath::new("../../../asdf/asdf/dasf/script_objects.bin")), &data)?;
        writer.finalize()?;
        Ok(())
    }

    #[test]
    fn test_write_container_preserves_header_version_and_mount_point() -> Result<()> {
        let output_dir = Path::new("out/raw-header-test");
        fs::remove_dir_all(output_dir).ok();
        fs::create_dir_all(output_dir)?;

        let toc_path = output_dir.join("container.utoc");
        let container_id = FIoContainerId::from_name("container");
        let header = FIoContainerHeader::new(EIoContainerHeaderVersion::SoftPackageReferencesOffset, container_id);
        let mut writer = IoStoreWriter::with_container_header(&toc_path, EIoStoreTocVersion::PerfectHashWithOverflow, "../../../".into(), Some(header))?;
        let package_id = FPackageId::from_name("/Game/Test");
        let chunk_id = FIoChunkId::from_package_id(package_id, 0, EIoChunkType::ExportBundleData);
        writer.write_package_chunk(chunk_id, Some(UEPath::new("../../../SB/Content/Test.uasset")), &[1, 2, 3], &StoreEntry::default())?;
        writer.finalize()?;

        let container = crate::iostore::IoStoreContainer::open(&toc_path, Arc::new(Config::default()))?;
        assert_eq!(container.container_header_version(), Some(EIoContainerHeaderVersion::SoftPackageReferencesOffset));
        assert_eq!(container.mount_point(), "../../../");
        assert_eq!(container.container_id(), container_id);
        assert!(container.package_store_entry(package_id).is_some());
        Ok(())
    }
}
