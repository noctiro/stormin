pub mod username;
pub use username::UsernameGenerator;
pub mod password;
pub use password::ChineseSocialPasswordGenerator;
pub use password::RandomPasswordGenerator;
pub mod qqid;
pub use qqid::QQIDGenerator;