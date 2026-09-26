#!/usr/bin/env python3
"""Tests for scripts/board.py. Run: python3 -m unittest scripts/test_board.py"""

import json
import os
import re
import struct
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import board  # noqa: E402


def png(path, rgb=(200, 100, 50)):
    """Writes a valid 2x2 RGB PNG."""
    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\x00" + bytes(rgb) * 2 for _ in range(2))
    data = (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 2, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw))
            + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(data)


class BoardTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        root = self.tmp.name
        self.gallery = os.path.join(root, "gallery-123")
        self.targets = os.path.join(root, "concepts")
        self.out = os.path.join(root, "board", "index.html")
        os.makedirs(self.gallery)
        os.makedirs(self.targets)
        for vid, stem, _ in board.VIEWS:
            png(os.path.join(self.targets, stem + ".png"))
            if vid != "T07":  # one view missing from the run
                png(os.path.join(self.gallery, stem + ".png"))
                png(os.path.join(self.gallery, stem + "-grey.png"), (90, 90, 90))

    def tearDown(self):
        self.tmp.cleanup()

    def page(self):
        with open(self.out, encoding="utf-8") as f:
            return f.read()

    def test_rows_pair_each_target_with_its_shot_by_relative_path(self):
        board.build(self.gallery, self.out, self.targets)
        page = self.page()
        self.assertEqual(page.count('<section class="view"'), 12)
        for vid, stem, _ in board.VIEWS:
            self.assertIn(f'data-id="{vid}"', page)
            self.assertIn(f'src="img/target-{stem}.png"', page)
        # Copies sit beside the page, so the folder stands alone.
        img = os.path.join(os.path.dirname(self.out), "img")
        self.assertTrue(os.path.isfile(os.path.join(img, "game-T01-spawn-vista.png")))
        self.assertTrue(os.path.isfile(os.path.join(img, "game-T01-spawn-vista-grey.png")))
        self.assertIn('data-grey="img/game-T01-spawn-vista-grey.png"', page)
        # The missing shot shows as missing rather than breaking the page.
        t07 = page.split('data-id="T07"')[1].split("</section>")[0]
        self.assertIn('class="missing"', t07)
        self.assertIn('src="img/target-T07-shield-break.png"', t07)

    def test_scores_notes_and_copy_are_on_the_page(self):
        board.build(self.gallery, self.out, self.targets)
        page = self.page()
        self.assertEqual(len(re.findall(r'type="radio" name="score-T01"', page)), 5)
        self.assertEqual(page.count("<textarea placeholder=\"Notes for"), 12)
        self.assertIn('id="copy"', page)
        self.assertIn('id="grey-all"', page)
        self.assertIn("navigator.clipboard", page)

    def test_the_page_makes_no_network_requests(self):
        board.build(self.gallery, self.out, self.targets)
        page = self.page()
        self.assertIsNone(re.search(r"(src|href)=\"(https?:)?//", page))
        self.assertNotIn("@import", page)
        self.assertNotIn("fetch(", page)

    def test_inline_embeds_every_image(self):
        board.build(self.gallery, self.out, self.targets, inline=True)
        page = self.page()
        self.assertNotIn('src="img/', page)
        self.assertEqual(page.count('src="data:image/png;base64,'), 11 + 12)

    def test_manifest_titles_and_records_are_used(self):
        views = [{"id": "T01", "name": "T01-spawn-vista", "title": "From the manifest",
                  "shots": 1, "hits": 1, "kills": 0, "missed": False, "rejected": []}]
        with open(os.path.join(self.gallery, "summary.json"), "w", encoding="utf-8") as f:
            json.dump({"commit": "abc1234", "scenario": {"views": views}}, f)
        board.build(self.gallery, self.out, self.targets)
        page = self.page()
        self.assertIn("From the manifest", page)
        self.assertIn("1 shots, 1 hits", page)
        self.assertIn("abc1234", page)
        # Views the manifest leaves out still get their rows.
        self.assertEqual(page.count('<section class="view"'), 12)

    def test_cli_writes_the_page(self):
        code = board.main([self.gallery, self.out, "--targets", self.targets])
        self.assertEqual(code, 0)
        self.assertTrue(os.path.isfile(self.out))
        self.assertEqual(board.main([]), 2)


if __name__ == "__main__":
    unittest.main()
