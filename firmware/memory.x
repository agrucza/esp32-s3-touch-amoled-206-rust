/* ESP32-S3 memory map - Waveshare ESP32-S3-Touch-AMOLED-2.06
 *
 * !! REFERENCE ONLY - NOT ACTIVE !!
 * This file is NOT copied to the build output. esp-hal provides its own
 * memory.x with the correct region names (irom_seg, drom_seg, etc.) that
 * the xtensa-lx-rt linker scripts expect.
 *
 * When we need custom regions (e.g. placing the display framebuffer in PSRAM),
 * we will re-add build.rs and rewrite this file with the correct naming
 * convention before activating it.
 *
 *
 * Hardware: 32 MB QIO Flash, 8 MB OPI PSRAM
 * Partition table: large_littlefs_32MB.csv
 *   App partition (ota_0): offset 0x10000, size 0x480000 (4.5 MB)
 *
 * This file is INCLUDE'd by esp-hal's linkall.x linker script.
 * It is copied into OUT_DIR by build.rs so the linker can find it.
 *
 * NOTE - PSRAM:
 *   The 8 MB OPI PSRAM virtual address is configured by esp-hal at runtime
 *   via the MMU. Do NOT define a PSRAM linker region here - the address is
 *   not fixed at link time. Instead use:
 *     esp_hal::psram::psram_raw_parts()   → raw pointer + length
 *   and build an allocator on top of it when needed (display framebuffer etc.)
 */

MEMORY {
    /* -------------------------------------------------------------------------
     * Internal SRAM - 320 KB
     * IRAM = instruction tightly-coupled memory (code that must run fast)
     * DRAM = data RAM (stack, static variables, heap if enabled)
     * ------------------------------------------------------------------------- */
    IRAM     (RWX) : ORIGIN = 0x40370000, LENGTH = 0x50000
    DRAM     (RW)  : ORIGIN = 0x3FC88000, LENGTH = 0x50000

    /* -------------------------------------------------------------------------
     * Internal Flash - cache-mapped windows
     *
     * App partition (ota_0) size = 0x480000 (4.5 MB).
     * The 0x20 offset skips the ESP image header at the start of the partition.
     *
     * IROM = instruction fetches from flash (read-execute, via I-cache)
     * DROM = read-only data in flash (.rodata constants, via D-cache)
     * ------------------------------------------------------------------------- */
    IROM     (RX)  : ORIGIN = 0x42000020, LENGTH = 0x480000 - 0x20
    DROM     (R)   : ORIGIN = 0x3C000020, LENGTH = 0x480000 - 0x20

    /* -------------------------------------------------------------------------
     * RTC fast memory - 8 KB
     * Survives deep sleep. Useful for wake stubs and retained variables.
     * ------------------------------------------------------------------------- */
    RTC_FAST (RWX) : ORIGIN = 0x600FE000, LENGTH = 0x2000
}
