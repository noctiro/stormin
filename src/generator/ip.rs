
use rand::RngCore;
use std::net::{Ipv4Addr, Ipv6Addr};

/// Generate a random IPv4 address as a String
/// 
/// # Arguments
/// * `rng` - Any mutable reference that implements the RngCore trait
///
/// # Returns
/// * A String representation of a random IPv4 address
pub fn generate_ipv4<T: RngCore>(rng: &mut T) -> String {
    // Generate a random u32 and convert it to IPv4
    let ip_int: u32 = rng.next_u32();
    
    // Create an IPv4 address from the u32
    let ip = Ipv4Addr::from(ip_int);
    
    // Convert to string representation
    ip.to_string()
}

/// Generate a random IPv6 address as a String
///
/// # Arguments
/// * `rng` - Any mutable reference that implements the RngCore trait
///
/// # Returns
/// * A String representation of a random IPv6 address
pub fn generate_ipv6<T: RngCore>(rng: &mut T) -> String {
    let mut bytes = [0u8; 16];
    rng.fill_bytes(&mut bytes);

    let ip = Ipv6Addr::from(bytes);
    ip.to_string()
}
