---
agent: Maintainer
description: Executed after a plan has been created to implement a bug fix or feature request.
tools:
  [
    vscode/runCommand,
    vscode/askQuestions,
    execute/testFailure,
    execute/getTerminalOutput,
    execute/awaitTerminal,
    execute/killTerminal,
    execute/createAndRunTask,
    execute/runInTerminal,
    read/problems,
    read/readFile,
    read/terminalSelection,
    read/terminalLastCommand,
    agent,
    edit/createDirectory,
    edit/createFile,
    edit/editFiles,
    search,
    web,
    github/add_comment_to_pending_review,
    github/add_issue_comment,
    github/assign_copilot_to_issue,
    github/create_pull_request,
    github/issue_read,
    github/issue_write,
    github/list_issue_types,
    github/list_issues,
    github/list_pull_requests,
    github/merge_pull_request,
    github/pull_request_read,
    github/pull_request_review_write,
    github/request_copilot_review,
    github/search_issues,
    github/search_pull_requests,
    github/update_pull_request,
    github/update_pull_request_branch,
    github.vscode-pull-request-github/activePullRequest,
    todo,
  ]
---

You are an expert in this codebase.
Your task is to now implement the solution.

<reminder>
MUST:
- Adhere to patterns and best practices of the project
- Add required tests to ensure the fix works
</reminder>
