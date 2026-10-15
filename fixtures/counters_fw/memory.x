MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 128K
  RAM : ORIGIN = 0x20000000, LENGTH = 32K
  BKPSRAM : ORIGIN = 0x38800000, LENGTH = 4K
}

/* The GNU build id in flash, so host tools can tell which firmware runs */
SECTIONS
{
  .note.gnu.build-id : ALIGN(4)
  {
    __build_id_start = .;
    KEEP(*(.note.gnu.build-id));
    __build_id_end = .;
  } > FLASH
} INSERT AFTER .rodata;
