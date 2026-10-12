# First-run welcome implementation

Frozen implementation plan for the design approved on 2026-10-12.

1. Add a self-contained welcome component and token-based animation stylesheet,
   including skip, a cleaned-up timer, and reduced-motion handling.
2. Render it before the existing wizard from the auth page; reserve a normal-flow
   language header and give intro/wizard the remaining viewport height.
3. Remove the preview area count, rename the fourth step in all locales, and make
   the two-column wizard stack on narrower windows. Match currency triggers to
   Select styling without replacing searchable currency selection.
4. Separate saved account identity from import dialog visibility. Closing import
   only changes visibility. Reopening reuses the account. Provide explicit entry
   to the application after account creation and prevent editing saved fields.
5. Translate new strings in all ten auth locales, update the first-run README,
   inspect the diff, run the repository gates, and exercise the UI with isolated
   data. Component/hook tests remain excluded by the repository testing policy.
