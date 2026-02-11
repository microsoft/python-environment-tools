---
agent: Maintainer
description: Implement a bug fix or feature.
tools:
  [
    vscode/runCommand,
    vscode/askQuestions,
    execute/testFailure,
    execute/getTerminalOutput,
    execute/runInTerminal,
    read/problems,
    read/readFile,
    read/terminalLastCommand,
    agent,
    edit/createFile,
    edit/editFiles,
    search,
    web/fetch,
    github/create_pull_request,
    github/issue_read,
    github/search_issues,
    github/request_copilot_review,
    todo,
  ]
---

Implement the solution based on the plan or user request.

1. Make focused code changes following project patterns
2. Add or update tests as needed
3. Run `cargo fmt --all` and `cargo clippy --all -- -D warnings` before committing
4. Create a PR when complete

Keep changes minimal and focused on the task.
