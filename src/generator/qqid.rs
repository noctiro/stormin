use rand::RngCore;

pub fn generate_qq_id<T: RngCore>(rng: &mut T) -> String {
    let length = rng.next_u32() as usize % 7 + 5; // 5..=11
    let qq_id: String = (0..length)
        .map(|_| (rng.next_u32() % 10).to_string())
        .collect();
    qq_id
}