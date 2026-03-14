# Changelog Rule

After completing each task, prepend a short entry to the top of `CHANGELOG.md` in the project root describing what was done. Each entry should include today's date and a brief description. Format: `- YYYY-MM-DD: description`. Keep it really simple, one line max. Also add codebase length to the end, example (14.3k loc). Do not type date every time — only when it changed and it's another day.

To count lines of code, run: `find src -name '*.rs' | xargs wc -l | tail -1`
