use rand::RngCore;

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
    
    // Generate two 32-bit random numbers for the remaining 8 digits
    // Each random number provides enough entropy for 4 digits
    let rand1 = rng.next_u32();
    let rand2 = rng.next_u32();
    
    // Fill remaining 8 digits using bitwise operations
    // Each operation extracts a single decimal digit (0-9) using modulo 10
    // This method avoids division and multiplication operations
    buffer[3] = (rand1 & 0xF) as u8 % 10 + ASCII_ZERO;
    buffer[4] = ((rand1 >> 4) & 0xF) as u8 % 10 + ASCII_ZERO;
    buffer[5] = ((rand1 >> 8) & 0xF) as u8 % 10 + ASCII_ZERO;
    buffer[6] = ((rand1 >> 12) & 0xF) as u8 % 10 + ASCII_ZERO;
    buffer[7] = ((rand1 >> 16) & 0xF) as u8 % 10 + ASCII_ZERO;
    buffer[8] = (rand2 & 0xF) as u8 % 10 + ASCII_ZERO;
    buffer[9] = ((rand2 >> 4) & 0xF) as u8 % 10 + ASCII_ZERO;
    buffer[10] = ((rand2 >> 8) & 0xF) as u8 % 10 + ASCII_ZERO;
    
    // Directly convert buffer to String, skipping UTF-8 validation
    // Safe because we only use ASCII digits 0-9
    unsafe { String::from_utf8_unchecked(buffer.to_vec()) }
}
