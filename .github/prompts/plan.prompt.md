---
agent: Maintainer
description: Create an implementation plan for a bug fix or feature.
tools:
  [
    vscode/askQuestions,
    read/problems,
    read/readFile,
    agent,
    search,
    web/fetch,
    github/issue_read,
    github/list_issues,
    github/pull_request_read,
    github/search_issues,
    todo,
  ]
---

Create an implementation plan. If a number is provided, treat it as a GitHub issue number.

1. Read the issue description and relevant code
2. Identify root cause (bugs) or clarify requirements (features)
3. Outline implementation steps following project patterns

Output a markdown plan with:

- **Overview**: What needs to be done?
- **Problem**: Root cause analysis (bugs only)
- **Solution**: Approach and key changes needed
- **Implementation Steps**: Ordered list of steps to complete the work

Do not make code changes.
