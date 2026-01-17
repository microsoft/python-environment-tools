// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

fn main() {
    #[cfg(target_os = "windows")]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.1.0".to_string());

        let mut res = winresource::WindowsResource::new();
        res.set("ProductName", "Python Environment Tools");
        res.set("FileDescription", "Python Environment Tools");
        res.set("CompanyName", "Microsoft Corporation");
        res.set(
            "LegalCopyright",
            "Copyright (c) Microsoft Corporation. All rights reserved.",
        );
        res.set("OriginalFilename", "pet.exe");
        res.set("InternalName", "pet");
        res.set("FileVersion", &version);
        res.set("ProductVersion", &version);
        res.compile().expect("Failed to compile Windows resources");
    }
}
