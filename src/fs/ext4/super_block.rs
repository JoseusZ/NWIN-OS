#[derive(Debug, Clone, Copy)]
#[repr(packed)]
pub struct Ext4SuperBlock {
    pub s_inodes_count: u32,      // Total de inodos
    pub s_blocks_count_lo: u32,   // Total de bloques
    pub s_r_blocks_count_lo: u32,
    pub s_free_blocks_count_lo: u32,
    pub s_free_inodes_count: u32,
    pub s_first_data_block: u32,
    pub s_log_block_size: u32,    // ¡Clave! block_size = 1024 << s_log_block_size
    pub s_log_cluster_size: u32,
    pub s_blocks_per_group: u32,
    pub s_clusters_per_group: u32,
    pub s_inodes_per_group: u32,
    pub s_mtime: u32,
    pub s_wtime: u32,
    pub s_mnt_count: u16,
    pub s_max_mnt_count: u16,
    pub s_magic: u16,             // Debe ser SIEMPRE 0xEF53
    pub s_state: u16,
    pub s_errors: u16,
    pub s_minor_rev_level: u16,
    pub s_lastcheck: u32,
    pub s_checkinterval: u32,
    pub s_creator_os: u32,
    pub s_rev_level: u32,
    pub s_def_resuid: u16,
    pub s_def_resgid: u16,
    // (Ext4 tiene muchos más campos, pero estos primeros 84 bytes son los esenciales para montar)
}

impl Ext4SuperBlock {
    /// Calcula el tamaño real de un bloque en bytes (usualmente 4096)
    pub fn block_size(&self) -> u64 {
        let log_size = { self.s_log_block_size };
        1024 << log_size
    }
}