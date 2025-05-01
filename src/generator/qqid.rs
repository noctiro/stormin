use rand::{rngs::{SmallRng, ThreadRng}, Rng, SeedableRng};


pub struct QQIDGenerator {
    rng: SmallRng,
}

impl QQIDGenerator {
    pub fn new() -> Self {
        Self {
            rng: SmallRng::seed_from_u64(ThreadRng::default().random()),
        }
    }

    pub fn generate_qq_id(&mut self) -> String {
        let length = self.rng.random_range(5..=11);
        let qq_id: String = (0..length)
            .map(|_| self.rng.random_range(0..10).to_string())
            .collect();
        qq_id
    }
}
