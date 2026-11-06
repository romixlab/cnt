MEMORY
{
  FLASH : ORIGIN = 0x08000000, LENGTH = 1M
  /* AXI SRAM */
  RAM : ORIGIN = 0x24000000, LENGTH = 320K
  /* RTC_BKP0R..3R: the BKP counters, kept across resets (src/main.rs enables write access) */
  BKP_REGS : ORIGIN = 0x58004050, LENGTH = 16
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
