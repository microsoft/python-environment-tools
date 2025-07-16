use std::fs;
use pet_uv::list_uv_virtual_envs_paths;

#[test]
fn test_uv_environment_discovery() {
    // Set up a temporary UV cache structure
    let temp_dir = std::env::temp_dir().join("test_pet_uv_integration");
    let cache_dir = temp_dir.join("uv");
    let env_dir = cache_dir.join("environments-v2");
    let test_env = env_dir.join("my-project-abc123-py3.12");
    let bin_dir = test_env.join("bin");
    
    // Create the directory structure
    fs::create_dir_all(&bin_dir).unwrap();
    
    // Create python executable and activate script to make it look like a virtual environment
    let python_exe = bin_dir.join("python");
    fs::write(&python_exe, "#!/bin/bash\necho 'python'").unwrap();
    let activate_script = bin_dir.join("activate");
    fs::write(&activate_script, "# Activate script").unwrap();
    
    // Test UV path discovery
    let uv_paths = list_uv_virtual_envs_paths(
        Some(cache_dir.to_string_lossy().to_string()),
        None,
        None,
    );
    
    // Verify that our test environment is discovered
    assert!(uv_paths.contains(&test_env), "UV environment should be discovered: {:?}", uv_paths);
    
    // Clean up
    fs::remove_dir_all(&temp_dir).ok();
}