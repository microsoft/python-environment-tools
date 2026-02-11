---
agent: Maintainer
description: Root cause analysis for a bug or issue.
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

Analyze the bug or issue. If a number is provided, treat it as a GitHub issue number.

1. Read the issue description and relevant code
2. Identify the root cause (for bugs) or clarify requirements (for features)
3. Determine if this is by-design behavior or an actual bug

Output a markdown summary with:

- **Overview**: What's the issue?
- **Root Cause**: Why is this happening? (bugs only)
- **Recommendation**: Should this be fixed? What are the risks?

Do not make code changes.
