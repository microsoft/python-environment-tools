// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

use pet_core::python_environment::PythonEnvironment;

pub trait Cache {
    fn get<P: AsRef<P>>(&self, executable: P) -> Option<PythonEnvironment>;
    fn set<P: AsRef<P>>(&self, environment: PythonEnvironment);
}
