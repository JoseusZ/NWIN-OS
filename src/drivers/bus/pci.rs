// src/drivers/pci.rs
use x86_64::instructions::port::Port;
use alloc::vec::Vec;

const PCI_CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const PCI_CONFIG_DATA_PORT: u16 = 0xCFC;

#[derive(Debug, PartialEq)]
pub enum Vendor {
    Intel,
    Amd,
    Nvidia,
    Qemu,
    Unknown(u32),
}

impl Vendor {
    pub fn new(id: u32) -> Self {
        match id {
            0x8086 => Self::Intel,
            0x1022 => Self::Amd,
            0x10DE => Self::Nvidia,
            0x1234 => Self::Qemu,
            _ => Self::Unknown(id),
        }
    }

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Unknown(id) => *id != 0xFFFF,
            _ => true,
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum DeviceType {
    SataController,
    IdeController,
    EthernetController,
    VgaCompatibleController,
    HostBridge,
    IsaBridge,
    Unknown(u32, u32),
}

impl DeviceType {
    pub fn new(base_class: u32, sub_class: u32) -> Self {
        match (base_class, sub_class) {
            (0x01, 0x01) => DeviceType::IdeController,
            (0x01, 0x06) => DeviceType::SataController,
            (0x02, 0x00) => DeviceType::EthernetController,
            (0x03, 0x00) => DeviceType::VgaCompatibleController,
            (0x06, 0x00) => DeviceType::HostBridge,
            (0x06, 0x01) => DeviceType::IsaBridge,
            _ => DeviceType::Unknown(base_class, sub_class),
        }
    }
}

pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor: Vendor,
    pub device_type: DeviceType,
}

impl PciDevice {

    pub unsafe fn write_u32(bus: u8, device: u8, func: u8, offset: u8, value: u32) {
        let address = (bus as u32) << 16
            | (device as u32) << 11
            | (func as u32) << 8
            | (offset as u32 & 0xFC)
            | 0x80000000;

        let mut addr_port = Port::<u32>::new(PCI_CONFIG_ADDRESS_PORT);
        let mut data_port = Port::<u32>::new(PCI_CONFIG_DATA_PORT);

        addr_port.write(address);
        data_port.write(value);
    }


    pub fn enable_mmio(&self) {
        unsafe {
            let command = Self::read_u32(self.bus, self.device, self.function, 0x04);
            Self::write_u32(self.bus, self.device, self.function, 0x04, command | (1 << 1));
        }
    }

    pub fn enable_bus_mastering(&self) {
        unsafe {
            let command = Self::read_u32(self.bus, self.device, self.function, 0x04);
            Self::write_u32(self.bus, self.device, self.function, 0x04, command | (1 << 2));
        }
    }

    pub fn get_bar5(&self) -> u32 {
        let bar_value = unsafe { Self::read_u32(self.bus, self.device, self.function, 0x24) };
        // Limpiamos los 4 bits más bajos, ya que son banderas de información, no parte de la dirección.
        bar_value & 0xFFFFFFF0 
    }

    pub unsafe fn read_u32(bus: u8, device: u8, func: u8, offset: u8) -> u32 {
        let address = (bus as u32) << 16
            | (device as u32) << 11
            | (func as u32) << 8
            | (offset as u32 & 0xFC)
            | 0x80000000;

        let mut addr_port = Port::<u32>::new(PCI_CONFIG_ADDRESS_PORT);
        let mut data_port = Port::<u32>::new(PCI_CONFIG_DATA_PORT);

        addr_port.write(address);
        data_port.read()
    }

    pub fn new(bus: u8, device: u8, function: u8) -> Option<Self> {
        unsafe {
            // Offset 0x00 contiene el Vendor ID (primeros 16 bits)
            let vendor_id = Self::read_u32(bus, device, function, 0x00) & 0xFFFF;
            let vendor = Vendor::new(vendor_id);

            if !vendor.is_valid() {
                return None;
            }

            // Offset 0x08 contiene el Class Code y Subclass Code en los bits altos
            let class_info = Self::read_u32(bus, device, function, 0x08);
            let base_class = (class_info >> 24) & 0xFF;
            let sub_class = (class_info >> 16) & 0xFF;
            let device_type = DeviceType::new(base_class, sub_class);

            Some(Self {
                bus,
                device,
                function,
                vendor,
                device_type,
            })
        }
    }

    pub fn has_multiple_functions(bus: u8, device: u8) -> bool {
        unsafe {
            let header_type = (Self::read_u32(bus, device, 0, 0x0C) >> 16) & 0xFF;
            (header_type & 0x80) != 0
        }
    }
}

/// Escanea el bus PCI utilizando el método de fuerza bruta (Buses 0-255, Dispositivos 0-31)
pub fn scan_pci_bus() -> Vec<PciDevice> {
    let mut devices = Vec::new();

    crate::println!("[PCI] Escaneando hardware...");

    for bus in 0..=255 {
        for device in 0..32 {
            // Verificamos si existe la función 0. Si no existe, no hay dispositivo.
            if PciDevice::new(bus, device, 0).is_some() {
                
                let multiple_functions = PciDevice::has_multiple_functions(bus, device);
                let functions_to_check = if multiple_functions { 8 } else { 1 };

                for function in 0..functions_to_check {
                    if let Some(func_dev) = PciDevice::new(bus, device, function) {
                        crate::println!(
                            "  --> Detectado: [{:?}] {:?}", 
                            func_dev.vendor, 
                            func_dev.device_type
                        );
                        devices.push(func_dev);
                    }
                }
            }
        }
    }
    
    crate::println!("[PCI] Escaneo completado. {} dispositivos encontrados.", devices.len());
    devices
}

// ====================================================================
// INICIALIZACIÓN DEL SUBSISTEMA PCI
// ====================================================================

/// Inicializa el bus PCI, escanea los dispositivos y devuelve la 
/// dirección de memoria (BAR5) de la controladora SATA si existe.
pub fn init() -> Option<u32> {
    let pci_devices = scan_pci_bus();
    let mut ahci_base_address: Option<u32> = None;

    for dev in pci_devices.iter() {
        if dev.device_type == DeviceType::SataController {
            let bar5 = dev.get_bar5();
            ahci_base_address = Some(bar5);
            
            dev.enable_mmio();
            dev.enable_bus_mastering();
            
            crate::serial_println!("[SATA] Controladora detectada en Bus {}, Dispositivo {}", dev.bus, dev.device);
            crate::serial_println!("[SATA] Registros AHCI mapeados en memoria fisica: {:#010x}", bar5);
            crate::serial_println!("[SATA] MMIO y Bus Mastering HABILITADOS.");
            crate::drivers::block::ahci::init(bar5);
            
            break; 
        }
    }

    if ahci_base_address.is_none() {
        crate::serial_println!("[WARNING] No se encontro ninguna controladora SATA en el bus PCI.");
    }

    ahci_base_address
}