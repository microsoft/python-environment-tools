# Support for

- PIPENV_PIPFILE
- PIPENV_MAX_DEPTH

# VirutalenvWrapper

- `find` is implemented
  Why? Ensure this same logic is implemented in the global lcoators
  Besides this logic might be wrong, as we might be marking `pipenv` as a `virtualenvwrapper` as a aresult of this logic.

# Support symlinks

- When we report an env, ensure we do not report the same env twice, by using the symlinks

# Priority

- Need to ship => List envs and compare against Python extension (compare exe & type) & send telemetry
- Resolve
- Cache

# On linux /usr/bin/python3.10 and /usr/bin/python3 have the same sys.prefix hence are treated as the same environments

# On linux /bin/python3.10 and /bin/python3 have the same sys.prefix hence are treated as the same environments

# Linux x64 homebrew

Install `brew install python@3.10`
The file `/usr/local/bin/python3.10` does not exist as per assumptions.
We need to ensure the files are verified before we reprot them.

# Pyenv manager not reported if there are no pyenv environments.

# Sometimes users have .venv files and `python` folders,

Ensure we take this into account.

# Ensure we handle searching folders with 10000s of files

# Mac test

- Write tests for CI, to ensure there are no mac global python where the executable does not point to anytihng in `framework.python...`
  The executable must be something in `/usr/local/bin/python?`
- Only one of the Mac-global python shoudl have two symlinks pointing to `/Library/Frameworks/Python.framework/Versions/Current/bin/python3?` exes

# Optimizations

- Avoid using `to_path` when using `read_dir`, where possible filter early
- Avoid reading metadata, try using `PathBuf.is_file...` etc where possible.
- Avoid checking if a file/dir exists, instead just open/read_dir and fail (thats one less I/O operation)

# Add `isWorkspaceLocator` to the Locator trait

This way we can create the locators in one place and re-use the list.

# Add CLI flag to wait for `conda_locator.find_with_conda_executable(conda_executable);` to complete so we can see if we missed anything

# Add CLI flag to print just summary, not everything, useful for debugging and see counts

# Add CLI flag to dump everything into a JSON file, useful for debugging and diagnostics

# Add CLI flag to resolve everything

# We need to handle the following scenarios

- Install commandline tools in mac
  /usr/bin/python3 points to that python env (/Library/Developer/CommandLineTools/usr/bin/python3)

- We cache this
  We have an env with /usr/bin/python3 and symlinks as /Library/Developer/CommandLineTools/usr/bin/python3

- Install X Code
  /usr/bin/python3 now points to another python env.
  /Applications/Xcode.app/Contents/Developer/usr/bin/python3

- Now when reading the cache, the executable `/usr/bin/python3`
  Does not point to the same place.
  Fortunately it doesn't work, as none of the locators directly support `/usr/bin/python3`
  We should
- Ensure the ctime and mtime of the main exe is captured
- Then when validating, check if that has changed, if not then continue with the validation.
- If it has changed, then skip validation, and let the usual locator code handle it.
<<<<<<< Updated upstream
=======


# Mutexes
* Review all of them
* Ensure we have simple and temporary lock and drop like
 list.lock().unwrap().push(1);
* Use them in if, short lived
  let item = list.lock().unwrap().pop();
  if let Some(item) = item {
      process_item(item);
  }
  
* Do not use in matches (as they live long, below is a anti-pattern)
  match list.lock().unwrap().pop() {
    Some(item) => process_item(item),
    None => (),
  }

  Or

if let Some(item) = list.lock().unwrap().pop() {
      process_item(item);
}
>>>>>>> Stashed changes
