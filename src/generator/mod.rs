pub mod user;
pub use user::UsernameGenerator;
pub mod password;
pub use password::ChineseSocialPasswordGenerator;
pub use password::RandomPasswordGenerator;
pub mod qqid;
pub use qqid::QQIDGenerator;