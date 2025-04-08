use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::PathBuf,
    ptr,
};

use tracing_subscriber::Layer;

use super::page::{ContiguousPages, MmapMut, PageAllocator};

const CLASS_PATH: &str = "/sys/class/u-dma-buf/udmabuf0";
const PAGE_SIZE_2MB: usize = 1 << 21;

pub(crate) struct UDmaBufAllocator {
    fd: File,
    offset: usize,
}

impl UDmaBufAllocator {
    pub(crate) fn open() -> io::Result<Self> {
        let fd = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_SYNC)
            .open("/dev/udmabuf0")?;

        Ok(Self { fd, offset: 0 })
    }

    fn size_total() -> io::Result<usize> {
        Self::read_attribute("size")
    }

    fn phys_addr() -> io::Result<u64> {
        Self::read_attribute("phys_addr")
    }

    fn read_attribute<T: std::str::FromStr>(attr: &str) -> io::Result<T>
    where
        T::Err: std::fmt::Display,
    {
        let path = PathBuf::from(CLASS_PATH).join(attr);
        let mut content = String::new();
        let _ignore = File::open(&path)?.read_to_string(&mut content)?;

        content.trim().parse().map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Failed to parse '{attr}': {e}"),
            )
        })
    }

    #[allow(clippy::cast_possible_wrap)]
    fn create(&mut self, n: usize) -> io::Result<MmapMut> {
        let size = PAGE_SIZE_2MB * n;
        let size_total = Self::size_total()?;
        let offset_in_bytes = self.offset * PAGE_SIZE_2MB;
        if self
            .offset
            .checked_add(n)
            .and_then(|x| x.checked_mul(PAGE_SIZE_2MB))
            .is_none_or(|x| x > size_total)
        {
            return Err(io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("Failed to allocate {n} pages"),
            ));
        }

        let fd = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_SYNC)
            .open("/dev/udmabuf0")?;

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED | libc::MAP_HUGETLB | libc::MAP_HUGE_2MB,
                fd.as_raw_fd(),
                offset_in_bytes as i64,
            )
        };

        if ptr.is_null() {
            return Err(io::Error::new(io::ErrorKind::Other, "Failed to map memory"));
        }

        self.offset += n;

        let mmap = MmapMut::new(ptr, size);

        Ok(mmap)
    }
}

impl<const N: usize> PageAllocator<N> for UDmaBufAllocator {
    fn alloc(&mut self) -> io::Result<ContiguousPages<N>> {
        self.create(N).map(ContiguousPages::new)
    }
}
