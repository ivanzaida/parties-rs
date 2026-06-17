use super::*;

#[test]
fn profiling_enabled_accepts_profile_arg() {
  assert!(profiling_arg_enabled(["--profile".to_owned()]));
  assert!(profiling_arg_enabled(["-profile=true".to_owned()]));
  assert!(!profiling_arg_enabled(["--profile=false".to_owned()]));
}
