use rand::RngCore;

// Lookup table to replace `% 10` for 4-bit values (0–15)
const MOD_10_TABLE: [u8; 16] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 1, 2, 3, 4, 5
];

// Mobile prefix segments for all Chinese carriers, stored as u16 for memory efficiency
static PREFIX_SEGMENTS: [u16; 53] = [
    130, 131, 132, 133, 134, 135, 136, 137, 138, 139,
    145, 146, 147, 148, 149,
    150, 151, 152, 153, 155, 156, 157, 158, 159,
    166, 167, 170, 171, 172, 173, 175, 176, 177,
    178, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189,
    190, 191, 192, 193, 195, 196, 197, 198, 199
];

// Pre-compute array size to avoid length calculation during runtime
const PREFIX_COUNT: usize = PREFIX_SEGMENTS.len();

// ASCII '0' constant for byte manipulation
const ASCII_ZERO: u8 = b'0';

// Fast path for phone number generation optimized for maximum throughput
#[inline(always)]
pub fn generate_cn_mobile<T: RngCore>(rng: &mut T) -> String {
    // Stack allocation of fixed-size buffer pre-filled with zeros
    let mut buffer = [0u8; 11];
    
    // Get a single random number for prefix selection and first digit group
    let rand_prefix = rng.next_u32();
    
    // Select prefix using fast modulo (prefix index is 0-48)
    // Using a direct cast is safe here since PREFIX_COUNT is small
    let prefix = PREFIX_SEGMENTS[rand_prefix as usize % PREFIX_COUNT];
    
    // First digit is always '1'
    buffer[0] = b'1';
    
    // Set second and third digits from prefix
    // Extract tens and ones digits directly
    buffer[1] = (prefix / 10 % 10) as u8 + ASCII_ZERO;
    buffer[2] = (prefix % 10) as u8 + ASCII_ZERO;
    
    // Generate 8 digits from a single u64
    let mut rand = rng.next_u64();

    for i in 3..11 {
        buffer[i] = MOD_10_TABLE[(rand & 0xF) as usize] + ASCII_ZERO;
        rand >>= 4;
    }
    
    // Directly convert buffer to String, skipping UTF-8 validation
    // Safe because we only use ASCII digits 0-9
    unsafe {
        // Allocate String manually without intermediate Vec
        let boxed = Box::new(buffer); // move stack array to heap
        String::from_raw_parts(Box::into_raw(boxed) as *mut u8, 11, 11)
    }
}
