use std::time::Duration;

use resonate_library::Waits;

pub(crate) const RELEASE: &str = "1c2d3e4f-5a6b-7c8d-9e0f-1a2b3c4d5e6f";

pub(crate) const RELEASE_GROUP: &str = "f5093c06-23e3-404f-aeaa-40f72885ee3a";

pub(crate) const ORBITERS: &str = "83d91898-7763-47d7-b03b-b92132375c47";

pub(crate) const ADA: &str = "aaaaaaaa-1111-2222-3333-444444444444";

pub(crate) const RELEASE_TRACK: &str = "3e7f0f5c-4d8a-4a7e-9b2a-0d3f6b6f9c11";

pub(crate) const HOURS: &str = "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d";

pub(crate) const HOURS_GROUP: &str = "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d";

pub(crate) const LIVE_GROUP: &str = "1b2c3d4e-5f6a-4b7c-8d8e-0f1a2b3c4d5e";

pub(crate) const BEST_OF_GROUP: &str = "2c3d4e5f-6a7b-4c8d-8e9f-1a2b3c4d5e6f";

pub(crate) const SCORE_GROUP: &str = "3d4e5f6a-7b8c-4d9e-8fa0-2b3c4d5e6f7a";

pub(crate) const SINGLE_GROUP: &str = "4e5f6a7b-8c9d-4eaf-8ab1-3c4d5e6f7a8b";

pub(crate) const BROADCAST_GROUP: &str = "5f6a7b8c-9dae-4fb0-8bc2-4d5e6f7a8b9c";

pub(crate) const RECORDING: &str = "b1a9c0de-1111-4222-8333-444455556666";

pub(crate) const ANOTHER_RECORDING: &str = "c2b8d1ef-2222-4333-8444-555566667777";

pub(crate) const CODE: &str = "GB-AYE-71-00195";

pub(crate) const ANOTHER_CODE: &str = "GB-AYE-71-00196";

pub(crate) const THE_FILES_LENGTH: Duration = Duration::from_millis(100);

pub(crate) const A_DAY: Duration = Duration::from_secs(24 * 60 * 60);

pub(crate) const WAITED: Waits = Waits {
    retry_after: A_DAY,
    refused_again_after: A_DAY,
    refresh_after: A_DAY,
};

pub(crate) const ASKED: i64 = 1_700_000_000_000_000_000;

pub(crate) const ANSWERED: i64 = 1_700_000_001_000_000_000;
