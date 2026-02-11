---
agent: Maintainer
description: Explain a feature or component in the codebase.
tools:
  [
    vscode/askQuestions,
    read/readFile,
    agent,
    search,
    web/fetch,
    github/search_issues,
    todo,
  ]
---

Explain the requested feature or component. Use diagrams where helpful.

1. Read relevant code and instruction files
2. Identify key patterns and architectural decisions
3. Trace data and control flow

Output a markdown explanation with:

- **Overview**: What does it do and why?
- **Key Components**: Main pieces involved
- **How It Works**: Data flow, control flow, integration points

Do not make code changes.
