from __future__ import annotations

import unittest

from scripts.changelog import extract_section_body, prepare_release


class ChangelogScriptTests(unittest.TestCase):
    def test_prepare_release_moves_unreleased_into_versioned_section(self) -> None:
        original = """# Changelog\n\n## Unreleased\n\n### Fixed\n- Smoothed Claude flapping.\n\n## [0.1.0] - 2026-03-27\n\n### Added\n- Initial release.\n"""

        updated = prepare_release(original, "0.1.1", "2026-03-28")

        self.assertIn("## Unreleased\n\n## [0.1.1] - 2026-03-28", updated)
        self.assertIn("### Fixed\n- Smoothed Claude flapping.", updated)
        self.assertIn("## [0.1.0] - 2026-03-27", updated)

    def test_prepare_release_accepts_bracketed_unreleased_heading(self) -> None:
        original = """# Changelog\n\n## [Unreleased]\n\n### Added\n- Added sounds.\n"""

        updated = prepare_release(original, "0.1.1", "2026-03-28")

        self.assertIn("## Unreleased\n\n## [0.1.1] - 2026-03-28", updated)
        self.assertIn("### Added\n- Added sounds.", updated)

    def test_extract_section_body_returns_requested_version_only(self) -> None:
        changelog = """# Changelog\n\n## Unreleased\n\n## [0.1.1] - 2026-03-28\n\n### Fixed\n- Smoothed Claude flapping.\n\n## [0.1.0] - 2026-03-27\n\n### Added\n- Initial release.\n"""

        body = extract_section_body(changelog, "0.1.1")

        self.assertEqual(body, "### Fixed\n- Smoothed Claude flapping.\n")


if __name__ == "__main__":
    unittest.main()
