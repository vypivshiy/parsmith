"""E2E integration tests for scraping sandboxes and real-world structural HTML fixtures."""

from __future__ import annotations

import pytest

from tests.integration.helpers import (
    FIXTURES_DIR,
    SCHEMAS_DIR,
    assert_cross_target_parity,
    assert_schema_lint_clean,
)

REAL_WORLD_SCHEMA_FILES: tuple[str, ...] = (
    "books_toscrape.kdl",
    "quotes_toscrape.kdl",
    "scrapethissite_hockey.kdl",
    "webscraper_ecommerce.kdl",
    "hackernews.kdl",
    "imdb_nextjs.kdl",
    "meta_extractor_css3.kdl",
    "meta_extractor_css4.kdl",
    "socials_extractor.kdl",
)


def _read_fixture(filename: str) -> str:
    """Read an HTML fixture file from tests/integration/fixtures/."""
    path = FIXTURES_DIR / filename
    assert path.is_file(), f"Missing HTML fixture: {path}"
    return path.read_text(encoding="utf-8")


@pytest.mark.parametrize("schema_file", REAL_WORLD_SCHEMA_FILES)
def test_real_world_schemas_are_lint_clean(schema_file: str) -> None:
    """Every sandbox and real-world schema passes parsmith.check() with zero errors and warnings."""
    assert_schema_lint_clean(schema_file)


class TestBooksToScrape:
    """books_toscrape.kdl on books_catalogue.html and books_product.html."""

    def test_main_catalogue_and_book_list(self) -> None:
        html = _read_fixture("books_catalogue.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "books_toscrape.kdl",
            "MainCatalogue",
            html,
        )
        book_list = assert_cross_target_parity(
            SCHEMAS_DIR / "books_toscrape.kdl",
            "Book",
            html,
        )
        assert result["books"] == book_list
        assert result == {
            "prev_page": "https://books.toscrape.com/catalogue/page-1.html",
            "next_page": "https://books.toscrape.com/catalogue/page-3.html",
            "curr_page": "https://books.toscrape.com/catalogue/page-2.html",
            "books": [
                {
                    "name": "A Light in the Attic",
                    "url": "https://books.toscrape.com/catalogue/a-light-in-the-attic_1000/index.html",
                    "image_url": "https://books.toscrape.com/media/cache/2c/da/2cdad67c44b002e7ead0cc35693c0e8b.jpg",
                    "rating": 3,
                    "price": 51.77,
                },
                {
                    "name": "Tipping the Velvet",
                    "url": "https://books.toscrape.com/catalogue/tipping-the-velvet_999/index.html",
                    "image_url": "https://books.toscrape.com/media/cache/26/0c/260c6ae16bce31c8f8c95daddd9f4a1c.jpg",
                    "rating": 1,
                    "price": 53.74,
                },
                {
                    "name": "Soumission",
                    "url": "https://books.toscrape.com/catalogue/soumission_998/index.html",
                    "image_url": "https://books.toscrape.com/media/cache/3e/ef/3eef99c9d9adef34639f510662022830.jpg",
                    "rating": 1,
                    "price": 50.1,
                },
            ],
        }

    def test_product_info_table(self) -> None:
        html = _read_fixture("books_product.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "books_toscrape.kdl",
            "ProductInfo",
            html,
        )
        assert result == {
            "upc": "a897fe39b1053632",
            "product_type": "Books",
            "price_excl_tax": 51.77,
            "price_incl_tax": 51.77,
            "tax": 0.0,
            "availability": 19,
            "number_of_reviews": 0,
        }


class TestQuotesToScrape:
    """quotes_toscrape.kdl (QuoteByHtml and QuotesByJs) on quotes_js.html."""

    def test_quotes_by_html(self) -> None:
        html = _read_fixture("quotes_js.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "quotes_toscrape.kdl",
            "QuoteByHtml",
            html,
        )
        items = assert_cross_target_parity(
            SCHEMAS_DIR / "quotes_toscrape.kdl",
            "QuotesHtmlItem",
            html,
        )
        assert result["quotes"] == items
        assert result == {
            "next_page": "/js/page/2/",
            "quotes": [
                {
                    "text": "“The world as we have created it is a process of our thinking. It cannot be changed without changing our thinking.”",
                    "author": "Albert Einstein",
                    "tags": ["change", "deep-thoughts", "thinking", "world"],
                },
                {
                    "text": "“It is our choices, Harry, that show what we truly are, far more than our abilities.”",
                    "author": "J.K. Rowling",
                    "tags": ["abilities", "choices"],
                },
                {
                    "text": "“There are only two ways to live your life. One is as though nothing is a miracle. The other is as though everything is a miracle.”",
                    "author": "Albert Einstein",
                    "tags": [
                        "inspirational",
                        "life",
                        "live",
                        "miracle",
                        "miracles",
                    ],
                },
            ],
        }

    def test_quotes_by_js(self) -> None:
        html = _read_fixture("quotes_js.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "quotes_toscrape.kdl",
            "QuotesByJs",
            html,
        )
        assert result == {
            "first_author": {
                "name": "Albert Einstein",
                "goodreads_links": "/author/show/9810.Albert_Einstein",
                "slug": "Albert-Einstein",
            },
            "second_author": {
                "name": "J.K. Rowling",
                "goodreads_links": "/author/show/1077326.J_K_Rowling",
                "slug": "J-K-Rowling",
            },
            "data": [
                {
                    "tags": ["change", "deep-thoughts", "thinking", "world"],
                    "author": {
                        "name": "Albert Einstein",
                        "goodreads_links": "/author/show/9810.Albert_Einstein",
                        "slug": "Albert-Einstein",
                    },
                    "text": "“The world as we have created it is a process of our thinking. It cannot be changed without changing our thinking.”",
                },
                {
                    "tags": ["abilities", "choices"],
                    "author": {
                        "name": "J.K. Rowling",
                        "goodreads_links": "/author/show/1077326.J_K_Rowling",
                        "slug": "J-K-Rowling",
                    },
                    "text": "“It is our choices, Harry, that show what we truly are, far more than our abilities.”",
                },
                {
                    "tags": [
                        "inspirational",
                        "life",
                        "live",
                        "miracle",
                        "miracles",
                    ],
                    "author": {
                        "name": "Albert Einstein",
                        "goodreads_links": "/author/show/9810.Albert_Einstein",
                        "slug": "Albert-Einstein",
                    },
                    "text": "“There are only two ways to live your life. One is as though nothing is a miracle. The other is as though everything is a miracle.”",
                },
            ],
        }


class TestScrapeThisSiteHockey:
    """scrapethissite_hockey.kdl on scrapethissite_hockey.html."""

    def test_hockey_page_and_teams(self) -> None:
        html = _read_fixture("scrapethissite_hockey.html")
        page = assert_cross_target_parity(
            SCHEMAS_DIR / "scrapethissite_hockey.kdl",
            "HockeyPage",
            html,
        )
        teams = assert_cross_target_parity(
            SCHEMAS_DIR / "scrapethissite_hockey.kdl",
            "HockeyTeam",
            html,
        )
        assert page["teams"] == teams
        assert "Hockey Teams: Forms, Searching and Pagination" in page["heading"]
        assert teams == [
            {
                "name": "Boston Bruins",
                "year": 1990,
                "wins": 44,
                "losses": 24,
                "ot_losses": 0,
                "win_pct": 0.55,
                "goals_for": 299,
                "goals_against": 264,
                "goal_diff": 35,
            },
            {
                "name": "Buffalo Sabres",
                "year": 1990,
                "wins": 31,
                "losses": 30,
                "ot_losses": 0,
                "win_pct": 0.388,
                "goals_for": 292,
                "goals_against": 296,
                "goal_diff": -4,
            },
            {
                "name": "San Jose Sharks",
                "year": 1999,
                "wins": 35,
                "losses": 37,
                "ot_losses": 10,
                "win_pct": 0.427,
                "goals_for": 214,
                "goals_against": 226,
                "goal_diff": -12,
            },
        ]


class TestWebScraperEcommerce:
    """webscraper_ecommerce.kdl on webscraper_ecommerce.html."""

    def test_ecommerce_page_and_products(self) -> None:
        html = _read_fixture("webscraper_ecommerce.html")
        page = assert_cross_target_parity(
            SCHEMAS_DIR / "webscraper_ecommerce.kdl",
            "EcommercePage",
            html,
        )
        products = assert_cross_target_parity(
            SCHEMAS_DIR / "webscraper_ecommerce.kdl",
            "EcommerceProduct",
            html,
        )
        assert page["category"] == "Computers / Laptops"
        assert page["products"] == products
        assert products == [
            {
                "title": "Asus VivoBook X441NA-GA190",
                "product_url": "/test-sites/e-commerce/allinone/product/545",
                "price": 295.99,
                "description": 'Asus VivoBook X441NA-GA190 Chocolate Black, 14", Celeron N3450, 4GB, 128GB SSD, Endless OS, ENG kbd',
                "review_count": 14,
                "rating_attr": 3,
                "stars_count": 3,
                "available_capacities": ["128", "256"],
            },
            {
                "title": "Prestigio SmartBook 133S Dark Grey",
                "product_url": "/test-sites/e-commerce/allinone/product/546",
                "price": 485.0,
                "description": 'Prestigio SmartBook 133S Dark Grey, 13.3" FHD IPS, Celeron N3350 1.1GHz, 4GB, 32GB, Windows 10 Pro',
                "review_count": 8,
                "rating_attr": 2,
                "stars_count": 2,
                "available_capacities": ["32", "64", "256"],
            },
            {
                "title": "Lenovo ThinkPad X1 Carbon",
                "product_url": "/test-sites/e-commerce/allinone/product/547",
                "price": 1399.0,
                "description": 'Lenovo ThinkPad X1 Carbon Gen 11, 14" WUXGA, Core i7-1355U, 16GB, 1TB SSD, Windows 11 Pro',
                "review_count": 29,
                "rating_attr": 5,
                "stars_count": 5,
                "available_capacities": ["512", "1024"],
            },
        ]


class TestHackerNews:
    """hackernews.kdl on hackernews.html and hackernews_discussion.html."""

    def test_main_page_submissions_and_job_post_fallbacks(self) -> None:
        html = _read_fixture("hackernews.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "hackernews.kdl",
            "MainPage",
            html,
        )
        assert result == {
            "news": [
                {
                    "title": "Show HN: Parsmith – Rust KDL 2.0 Schema Compiler",
                    "rank": 1,
                    "id": 40001,
                    "votelink": "https://news.ycombinator.com/vote?id=40001&how=up&goto=news",
                    "url": "https://parsmith.dev/blog/release-v2",
                    "site": "parsmith.dev",
                },
                {
                    "title": "Making SQLite 20% Faster",
                    "rank": 2,
                    "id": 40002,
                    "votelink": "https://news.ycombinator.com/vote?id=40002&how=up&goto=news",
                    "url": "https://sqlite.org/faster",
                    "site": "sqlite.org",
                },
                {
                    "title": "Acme Systems (YC W25) Is Hiring Systems Engineers",
                    "rank": 3,
                    "id": 40003,
                    "votelink": None,
                    "url": "https://ycombinator.com/companies/acme/jobs",
                    "site": None,
                },
            ],
            "ratings": [
                {
                    "score": 142,
                    "author": "https://news.ycombinator.com/user?id=alex",
                    "date": "2025-02-01T10:00:00 1738404000",
                    "date_text": "3 hours ago",
                    "url": "https://news.ycombinator.com/item?id=40001",
                    "comments": 87,
                },
                {
                    "score": 310,
                    "author": "https://news.ycombinator.com/user?id=drh",
                    "date": "2025-02-01T08:30:00 1738398600",
                    "date_text": "5 hours ago",
                    "url": "https://news.ycombinator.com/item?id=40002",
                    "comments": 115,
                },
                {
                    "score": 0,
                    "author": None,
                    "date": "2025-02-01T07:00:00 1738393200",
                    "date_text": "6 hours ago",
                    "url": "https://news.ycombinator.com/item?id=40003",
                    "comments": 0,
                },
            ],
        }

    def test_discussion_page_comment_tree_and_missing_reply(self) -> None:
        html = _read_fixture("hackernews_discussion.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "hackernews.kdl",
            "MainDiscussionPage",
            html,
        )
        assert result == {
            "title": "Show HN: Parsmith – Rust KDL 2.0 Schema Compiler",
            "url": "https://parsmith.dev/blog/release-v2",
            "score": 142,
            "user": "alex",
            "user_url": "https://news.ycombinator.com/user?id=alex",
            "date": "2025-02-01T10:00:00 1738404000",
            "date_text": "3 hours ago",
            "comments_count": 3,
            "comments": [
                {
                    "indent": 0,
                    "user": "alice_dev",
                    "user_url": "https://news.ycombinator.com/user?id=alice_dev",
                    "date": "2025-02-01T10:15:00 1738404900",
                    "date_text": "2 hours ago",
                    "text": "Love the algebraic AST design and zero-copy KDL 2.0 frontend!",
                    "reply": "https://news.ycombinator.com/reply?id=40010&goto=item%3Fid%3D40001%2340010",
                },
                {
                    "indent": 1,
                    "user": "alex",
                    "user_url": "https://news.ycombinator.com/user?id=alex",
                    "date": "2025-02-01T10:30:00 1738405800",
                    "date_text": "1 hour ago",
                    "text": "Thanks! We also added cross-target parity verification across Python and JS.",
                    "reply": "https://news.ycombinator.com/reply?id=40011&goto=item%3Fid%3D40001%2340011",
                },
                {
                    "indent": 2,
                    "user": "bob_ops",
                    "user_url": "https://news.ycombinator.com/user?id=bob_ops",
                    "date": "2025-02-01T11:00:00 1738407600",
                    "date_text": "30 minutes ago",
                    "text": "Locked archival subthread without reply link.",
                    "reply": None,
                },
            ],
        }


class TestImdbNextJs:
    """imdb_nextjs.kdl on imdb_nextjs.html."""

    def test_imdb_next_data_and_hybrid_page(self) -> None:
        html = _read_fixture("imdb_nextjs.html")
        next_data = assert_cross_target_parity(
            SCHEMAS_DIR / "imdb_nextjs.kdl",
            "ImdbNextData",
            html,
        )
        hybrid = assert_cross_target_parity(
            SCHEMAS_DIR / "imdb_nextjs.kdl",
            "ImdbHybridPage",
            html,
        )
        cards = assert_cross_target_parity(
            SCHEMAS_DIR / "imdb_nextjs.kdl",
            "ImdbHtmlCard",
            html,
        )
        assert hybrid["next_data"] == next_data
        assert hybrid["cards"] == cards
        assert hybrid["cards"] == [
            {
                "image": "https://m.media-amazon.com/images/M/matrix.jpg",
                "image_alt": "The Matrix Poster",
                "title": "The Matrix",
                "url": "https://www.imdb.com/title/tt0133093/?ref_=fn_tt_tt_1",
                "year": 1999,
            },
            {
                "image": "https://m.media-amazon.com/images/M/mrrobot.jpg",
                "image_alt": "Mr. Robot Poster",
                "title": "Mr. Robot",
                "url": "https://www.imdb.com/title/tt4158110/?ref_=fn_tt_tt_2",
                "year": 20152019,
            },
            {
                "image": None,
                "image_alt": None,
                "title": "Untitled Matrix Project",
                "url": "https://www.imdb.com/title/tt9999999/?ref_=fn_tt_tt_3",
                "year": None,
            },
        ]

        assert len(next_data["title_results"]) == 3
        assert next_data["top_result"] == next_data["title_results"][0]

        first = next_data["title_results"][0]
        assert first["id"] == "tt0133093"
        assert first["title_name_text"] == "The Matrix"
        assert first["series_id"] is None
        assert "undeclaredPosterKey" not in first["title_poster_image_model"]

        second = next_data["title_results"][1]
        assert second["id"] == "tt4652838"
        assert second["series_id"] == "tt4158110"
        assert second["series_name_text"] == "Mr. Robot"
        assert second["series_season_text"] == "1"
        assert second["series_episode_text"] == "1"

        third = next_data["title_results"][2]
        assert third["id"] == "tt9999999"
        assert third["title_poster_image_model"] is None
        assert "extraWireKey" not in third
        assert "extra_wire_key" not in third
        assert "internalScore" not in third
        assert "internal_score" not in third


class TestMetaExtractorCss3VsCss4:
    """meta_extractor_css3.kdl vs meta_extractor_css4.kdl on meta_opengraph.html."""

    def test_css3_and_css4_produce_identical_metadata(self) -> None:
        html = _read_fixture("meta_opengraph.html")
        res_css3 = assert_cross_target_parity(
            SCHEMAS_DIR / "meta_extractor_css3.kdl",
            "UniversalMetaExtractor",
            html,
        )
        res_css4 = assert_cross_target_parity(
            SCHEMAS_DIR / "meta_extractor_css4.kdl",
            "UniversalMetaExtractor",
            html,
        )
        assert res_css3 == res_css4
        assert res_css3 == {
            "title": "Parsmith Compiler Architecture & Benchmarks",
            "canonical": "https://parsmith.dev/docs/architecture",
            "favicon": "/favicon.ico",
            "rss_feeds": [
                "https://parsmith.dev/feed.xml",
                "https://parsmith.dev/atom.xml",
            ],
            "meta": {
                "description": "Universal KDL 2.0 schema compiler and multi-target code generator in Rust.",
                "keywords": ["rust", "kdl", "scraper", "codegen", "ast"],
                "author": "Alex",
                "viewport": "width=device-width, initial-scale=1.0",
            },
            "open_graph": {
                "title": "Parsmith Compiler Architecture",
                "description": "Compile declarative KDL 2.0 schemas to typed Python and TypeScript parsers.",
                "url": "https://parsmith.dev/docs/architecture",
                "image": "https://parsmith.dev/static/og-banner.png",
                "others": {
                    "og:type": "article",
                    "og:site_name": "Parsmith Docs",
                    "og:locale": "en_US",
                },
            },
            "twitter": {
                "card": "summary_large_image",
                "title": "Parsmith Compiler Architecture",
                "description": "Compile declarative KDL 2.0 schemas to typed Python and TypeScript parsers.",
                "image": "https://parsmith.dev/static/twitter-card.png",
                "others": {
                    "twitter:site": "@parsmith_dev",
                    "twitter:creator": "@alex_dev",
                },
            },
        }


class TestSocialsAndPlayerExtractor:
    """socials_extractor.kdl (SocialsAndContacts and PlayerConfig) on socials_player.html."""

    def test_socials_and_contacts(self) -> None:
        html = _read_fixture("socials_player.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "socials_extractor.kdl",
            "SocialsAndContacts",
            html,
        )
        git_links = assert_cross_target_parity(
            SCHEMAS_DIR / "socials_extractor.kdl",
            "Git",
            html,
        )
        socials_links = assert_cross_target_parity(
            SCHEMAS_DIR / "socials_extractor.kdl",
            "Socials",
            html,
        )
        contacts = assert_cross_target_parity(
            SCHEMAS_DIR / "socials_extractor.kdl",
            "Contacts",
            html,
        )
        assert result["git"] == git_links
        assert result["socials"] == socials_links
        assert result["contacts"] == contacts
        assert result == {
            "git": [
                "https://github.com/parsmith/parsmith",
                "https://gitlab.com/parsmith/core",
            ],
            "socials": [
                "https://twitter.com/parsmith_dev",
                "https://x.com/parsmith_dev/share",
                "https://www.tiktok.com/@parsmith/video/101",
                "https://www.youtube.com/embed/dQw4w9WgXcQ",
                "https://x.com/parsmith_news/feed.rss",
                "https://youtube.com/watch/channel.html",
            ],
            "contacts": {
                "phone": ["tel:+1-800-555-0199"],
                "email": [
                    "mailto:support@parsmith.dev",
                    "mailto:security@parsmith.dev",
                ],
            },
        }

    def test_player_config_raw_struct(self) -> None:
        html = _read_fixture("socials_player.html")
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "socials_extractor.kdl",
            "PlayerConfig",
            html,
        )
        assert result == {
            "player_id": "main_player",
            "stream_file": "/hls/master.m3u8",
        }
