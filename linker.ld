ENTRY(_start)

/* * Definimos los segmentos físicos (Program Headers) explícitamente.
 * Esto garantiza la seguridad W^X (Write XOR Execute) a nivel de binario ELF.
 * FLAGS: 1 = Ejecución, 2 = Escritura, 4 = Lectura
 */
PHDRS
{
    text    PT_LOAD    FLAGS(5); /* 4 (Lectura) + 1 (Ejecución) = 5 (RX) */
    rodata  PT_LOAD    FLAGS(4); /* 4 (Lectura) = 4 (R) */
    data    PT_LOAD    FLAGS(6); /* 4 (Lectura) + 2 (Escritura) = 6 (RW) */
}

SECTIONS
{
    /* Punto de anclaje: Inicio del Higher Half Kernel en x86_64 */
    . = 0xffffffff80200000;

    /* SECCIÓN 1: Código ejecutable del Kernel */
    .text : ALIGN(4096) {
        *(.text .text.*)
    } :text

    /* SECCIÓN 2: Datos de Solo Lectura (Constantes y Protocolo Limine) */
    .rodata : ALIGN(4096) {
        /* ¡VITAL! Las peticiones de Limine deben ir estrictamente PRIMERO */
        /* Esto evita que LLVM mezcle cadenas de texto de Rust entre las peticiones */
        KEEP(*(.requests_start_marker))
        KEEP(*(.requests))
        KEEP(*(.requests_end_marker))
        
        /* Después de los marcadores, ya podemos poner el resto de constantes de Rust */
        *(.rodata .rodata.*)
    } :rodata

    /* SECCIÓN 3: Datos Inicializados (Variables estáticas mutables) */
    .data : ALIGN(4096) {
        *(.data .data.*)
    } :data

    /* SECCIÓN 4: Datos No Inicializados (BSS) */
    .bss : ALIGN(4096) {
        *(COMMON)
        *(.bss .bss.*)
    } :data

    /* SECCIÓN 5: Limpieza profunda (Nivel de Producción) */
    /* Le decimos al linker que tire a la basura metadatos inútiles en bare-metal */
    /DISCARD/ : {
        *(.eh_frame)       /* Frames de excepciones de C++ / Rust (no usamos unwinding) */
        *(.note .note.*)   /* Notas del compilador y sistema anfitrión */
    }
}