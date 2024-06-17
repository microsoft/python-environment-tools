// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

pub trait Cache {
    fn get<P: AsRef<P>>(&self, executable: P) -> Option<PythonEnvironment>;
    fn set<P: AsRef<P>>(&self, environment: PythonEnvironment);
}
