## 1. Enhance Error Messages
- [ ] 1.1 Map clonedir_lib errors to user-friendly messages (e.g., detect permission denied)
- [ ] 1.2 Update CliError enum and Display for specificity

## 2. Add Destination Permission Validation
- [ ] 2.1 Implement write permission check for destination parent directory
- [ ] 2.2 Add error handling for permission failures

## 3. Expand Tests
- [ ] 3.1 Add integration tests for --dry-run and --verbose behavior
- [ ] 3.2 Add tests for error scenarios (permission denied, invalid paths)
- [ ] 3.3 Add test for confirmation prompt logic

## 4. Code Quality
- [ ] 4.1 Run rustfmt and clippy, fix issues
- [ ] 4.2 Ensure all tests pass

## 5. Validate and Archive
- [ ] 5.1 Run openspec validate --strict
- [ ] 5.2 Archive change after implementation