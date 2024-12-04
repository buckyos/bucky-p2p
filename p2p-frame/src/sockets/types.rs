
#[derive(Clone, Copy, PartialOrd, Ord, PartialEq, Eq)]
pub enum UpdateOuterResult {
    None,
    Update,
    Reset
}
