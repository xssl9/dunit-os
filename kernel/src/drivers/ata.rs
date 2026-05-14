use core::arch::asm;

const ATA_PRIMARY_IO: u16 = 0x1F0;
const ATA_PRIMARY_CONTROL: u16 = 0x3F6;

const ATA_REG_DATA: u16 = 0;
const ATA_REG_ERROR: u16 = 1;
const ATA_REG_SECTOR_COUNT: u16 = 2;
const ATA_REG_LBA_LOW: u16 = 3;
const ATA_REG_LBA_MID: u16 = 4;
const ATA_REG_LBA_HIGH: u16 = 5;
const ATA_REG_DRIVE: u16 = 6;
const ATA_REG_STATUS: u16 = 7;
const ATA_REG_COMMAND: u16 = 7;

const ATA_CMD_READ_PIO: u8 = 0x20;
const ATA_CMD_WRITE_PIO: u8 = 0x30;
const ATA_CMD_IDENTIFY: u8 = 0xEC;

const ATA_STATUS_BSY: u8 = 0x80;
const ATA_STATUS_DRQ: u8 = 0x08;
const ATA_STATUS_ERR: u8 = 0x01;

pub struct AtaDrive {
    base: u16,
    control: u16,
    slave: bool,
}

impl AtaDrive {
    pub fn new(primary: bool, slave: bool) -> Self {
        let (base, control) = if primary {
            (ATA_PRIMARY_IO, ATA_PRIMARY_CONTROL)
        } else {
            (0x170, 0x376)
        };
        
        Self { base, control, slave }
    }

    fn outb(&self, reg: u16, value: u8) {
        unsafe {
            asm!("out dx, al", in("dx") self.base + reg, in("al") value, options(nomem, nostack));
        }
    }

    fn inb(&self, reg: u16) -> u8 {
        let value: u8;
        unsafe {
            asm!("in al, dx", out("al") value, in("dx") self.base + reg, options(nomem, nostack));
        }
        value
    }

    fn inw(&self, reg: u16) -> u16 {
        let value: u16;
        unsafe {
            asm!("in ax, dx", out("ax") value, in("dx") self.base + reg, options(nomem, nostack));
        }
        value
    }

    fn outw(&self, reg: u16, value: u16) {
        unsafe {
            asm!("out dx, ax", in("dx") self.base + reg, in("ax") value, options(nomem, nostack));
        }
    }

    fn wait_ready(&self) {
        while (self.inb(ATA_REG_STATUS) & ATA_STATUS_BSY) != 0 {}
    }

    fn wait_drq(&self) -> bool {
        self.wait_ready();
        let status = self.inb(ATA_REG_STATUS);
        (status & ATA_STATUS_DRQ) != 0 && (status & ATA_STATUS_ERR) == 0
    }

    pub fn identify(&self) -> bool {
        self.outb(ATA_REG_DRIVE, if self.slave { 0xB0 } else { 0xA0 });
        self.outb(ATA_REG_SECTOR_COUNT, 0);
        self.outb(ATA_REG_LBA_LOW, 0);
        self.outb(ATA_REG_LBA_MID, 0);
        self.outb(ATA_REG_LBA_HIGH, 0);
        self.outb(ATA_REG_COMMAND, ATA_CMD_IDENTIFY);

        let status = self.inb(ATA_REG_STATUS);
        if status == 0 {
            return false;
        }

        self.wait_ready();
        
        let lba_mid = self.inb(ATA_REG_LBA_MID);
        let lba_high = self.inb(ATA_REG_LBA_HIGH);
        if lba_mid != 0 || lba_high != 0 {
            return false;
        }

        if !self.wait_drq() {
            return false;
        }

        for _ in 0..256 {
            let _ = self.inw(ATA_REG_DATA);
        }

        true
    }

    pub fn read_sector(&self, lba: u32, buffer: &mut [u8; 512]) -> bool {
        if buffer.len() != 512 {
            return false;
        }

        self.wait_ready();

        self.outb(ATA_REG_DRIVE, if self.slave { 0xF0 } else { 0xE0 } | ((lba >> 24) & 0x0F) as u8);
        self.outb(ATA_REG_SECTOR_COUNT, 1);
        self.outb(ATA_REG_LBA_LOW, (lba & 0xFF) as u8);
        self.outb(ATA_REG_LBA_MID, ((lba >> 8) & 0xFF) as u8);
        self.outb(ATA_REG_LBA_HIGH, ((lba >> 16) & 0xFF) as u8);
        self.outb(ATA_REG_COMMAND, ATA_CMD_READ_PIO);

        if !self.wait_drq() {
            return false;
        }

        for i in 0..256 {
            let word = self.inw(ATA_REG_DATA);
            buffer[i * 2] = (word & 0xFF) as u8;
            buffer[i * 2 + 1] = (word >> 8) as u8;
        }

        true
    }

    pub fn write_sector(&self, lba: u32, buffer: &[u8; 512]) -> bool {
        if buffer.len() != 512 {
            return false;
        }

        self.wait_ready();

        self.outb(ATA_REG_DRIVE, if self.slave { 0xF0 } else { 0xE0 } | ((lba >> 24) & 0x0F) as u8);
        self.outb(ATA_REG_SECTOR_COUNT, 1);
        self.outb(ATA_REG_LBA_LOW, (lba & 0xFF) as u8);
        self.outb(ATA_REG_LBA_MID, ((lba >> 8) & 0xFF) as u8);
        self.outb(ATA_REG_LBA_HIGH, ((lba >> 16) & 0xFF) as u8);
        self.outb(ATA_REG_COMMAND, ATA_CMD_WRITE_PIO);

        if !self.wait_drq() {
            return false;
        }

        for i in 0..256 {
            let word = buffer[i * 2] as u16 | ((buffer[i * 2 + 1] as u16) << 8);
            self.outw(ATA_REG_DATA, word);
        }

        self.wait_ready();
        true
    }
}

static mut PRIMARY_MASTER: Option<AtaDrive> = None;

pub fn init() {
    unsafe {
        let drive = AtaDrive::new(true, false);
        if drive.identify() {
            PRIMARY_MASTER = Some(drive);
        }
    }
}

pub fn get_primary_master() -> Option<&'static AtaDrive> {
    unsafe { PRIMARY_MASTER.as_ref() }
}
