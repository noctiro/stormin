use rand::RngCore;

// Static string slices to minimize memory allocations
static BROWSERS: [&str; 4] = ["Chrome", "Firefox", "Safari", "Edge"];
static OS: [&str; 8] = [
    "Windows NT",
    "Macintosh; Intel Mac OS X",
    "Linux",
    "Android",
    "iPhone OS",
    "X11; Ubuntu",
    "X11; Fedora",
    "Windows Phone",
];

pub fn generate_user_agent<T: RngCore>(rng: &mut T) -> String {
    // Pre-allocated buffer for better performance
    let mut buffer = Vec::with_capacity(128);

    // Generate random values for different UA components using bit manipulation
    let mut random_bytes = [0u8; 16];
    rng.fill_bytes(&mut random_bytes);

    // Use bit shifting to extract different components
    let browser_type = (random_bytes[0] & 0x3) as usize; // 2 bits for browser type (0-3)
    let major_version = ((random_bytes[1] >> 2) & 0x3F) + 70; // 6 bits for major version (70-133)
    let minor_version = random_bytes[2] & 0x3F; // 6 bits for minor version (0-63)
    let patch_version = random_bytes[3] & 0x3F; // 6 bits for patch version (0-63)

    let os_type = (random_bytes[4] & 0x7) as usize; // 3 bits for OS type (0-7)
    let os_version_major = (random_bytes[5] & 0x1F) + 5; // 5 bits for OS major version (5-36)
    let os_version_minor = random_bytes[6] & 0xF; // 4 bits for OS minor version (0-15)

    // Build the User Agent string using our pre-allocated buffer
    // Mozilla/5.0 prefix is common to most UAs
    buffer.extend_from_slice(b"Mozilla/5.0 (");

    // OS Information
    buffer.extend_from_slice(OS[os_type].as_bytes());

    if os_type == 0 || os_type == 7 {
        // Windows-based
        buffer.extend_from_slice(b" ");
        let version = ((os_version_major as u8) << 4) | (os_version_minor & 0xF);
        buffer.extend_from_slice(format!("{}.{}", version >> 4, version & 0xF).as_bytes());
    } else if os_type == 1 {
        // Mac OS X
        buffer.extend_from_slice(b" ");
        buffer.extend_from_slice(format!("{}_{}", os_version_major, os_version_minor).as_bytes());
    } else if os_type == 3 || os_type == 4 {
        // Mobile
        buffer.extend_from_slice(b" ");
        buffer.extend_from_slice(format!("{}.{}", os_version_major, os_version_minor).as_bytes());
    }

    buffer.extend_from_slice(b") ");

    // Webkit/KHTML/Gecko rendering engine info - depends on browser
    match browser_type {
        0 => {
            // Chrome
            // AppleWebKit/537.36 (KHTML, like Gecko) Chrome/xx.xx.xx Safari/537.36
            buffer.extend_from_slice(b"AppleWebKit/537.36 (KHTML, like Gecko) ");
            buffer.extend_from_slice(BROWSERS[browser_type].as_bytes());
            buffer.extend_from_slice(b"/");
            buffer.extend_from_slice(
                format!("{}.{}.{}", major_version, minor_version, patch_version).as_bytes(),
            );
            buffer.extend_from_slice(b" Safari/537.36");
        }
        1 => {
            // Firefox
            // Gecko/20100101 Firefox/xx.x
            buffer.extend_from_slice(b"Gecko/20100101 ");
            buffer.extend_from_slice(BROWSERS[browser_type].as_bytes());
            buffer.extend_from_slice(b"/");
            buffer.extend_from_slice(format!("{}.{}", major_version, minor_version).as_bytes());
        }
        2 => {
            // Safari
            // AppleWebKit/605.1.15 (KHTML, like Gecko) Version/x.x Safari/605.1.15
            buffer.extend_from_slice(b"AppleWebKit/605.1.15 (KHTML, like Gecko) Version/");
            buffer.extend_from_slice(
                format!("{}.{}", (os_version_major % 10) + 5, os_version_minor).as_bytes(),
            );
            buffer.extend_from_slice(b" ");
            buffer.extend_from_slice(BROWSERS[browser_type].as_bytes());
            buffer.extend_from_slice(b"/");
            buffer.extend_from_slice(format!("{}.{}.{}", 605, 1, 15).as_bytes());
        }
        _ => {
            // Edge
            // AppleWebKit/537.36 (KHTML, like Gecko) Chrome/xx.xx.xx Safari/537.36 Edg/xx.x.xxx
            buffer.extend_from_slice(b"AppleWebKit/537.36 (KHTML, like Gecko) ");
            buffer.extend_from_slice(b"Chrome/");
            buffer.extend_from_slice(
                format!("{}.{}.{}", major_version, minor_version, patch_version).as_bytes(),
            );
            buffer.extend_from_slice(b" Safari/537.36 Edg/");
            buffer.extend_from_slice(
                format!(
                    "{}.{}.{}",
                    (major_version - 30) % 50 + 80,
                    minor_version % 10,
                    patch_version
                )
                .as_bytes(),
            );
        }
    }

    // Add extra entropy with device pixel ratio for some browsers
    if browser_type == 0 && (random_bytes[9] & 0x1) == 1 {
        // Device memory only in Chrome
        let device_memory = 1 << (random_bytes[10] & 0x3); // 1, 2, 4, or 8
        buffer.extend_from_slice(format!(" device-memory/{}", device_memory).as_bytes());
    }

    // Convert buffer to String - safe because we only inserted valid UTF-8
    unsafe { String::from_utf8_unchecked(buffer) }
}
