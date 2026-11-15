import { describe, expect, it } from "vitest";
import { getPresetById } from "../presets";
import { testHtml } from "../runtime/wasmClient";
import {
  getHtmlTestActionState,
  selectDefaultStructName,
  summarizeHealthReport,
  type StructHealthReport,
  type StructInfo,
} from "../types";

const CATALOGUE_STRUCTS: StructInfo[] = [
  {
    name: "QuoteAuthor",
    type: "item",
    is_rest: false,
    referenced_by: ["QuoteCatalogue"],
  },
  {
    name: "QuoteCatalogue",
    type: "list",
    is_rest: false,
    referenced_by: [],
  },
];

const MIXED_STRUCTS: StructInfo[] = [
  {
    name: "QuoteAuthor",
    type: "item",
    is_rest: false,
    referenced_by: ["QuoteCatalogue"],
  },
  {
    name: "QuoteCatalogue",
    type: "list",
    is_rest: false,
    referenced_by: [],
  },
  {
    name: "QuotesApi",
    type: "rest",
    is_rest: true,
    referenced_by: [],
  },
];

const REST_ONLY_STRUCTS: StructInfo[] = [
  { name: "UserApi", type: "rest", is_rest: true },
];

describe("selectDefaultStructName", () => {
  it.each([
    [
      "auto-selects the primary (last) root non-REST struct when selection is omitted",
      CATALOGUE_STRUCTS,
      undefined,
      "QuoteCatalogue",
    ],
    [
      "auto-selects the primary (last) root non-REST struct when no struct is chosen",
      CATALOGUE_STRUCTS,
      null,
      "QuoteCatalogue",
    ],
    [
      "auto-selects the primary (last) root non-REST struct from mixed structs when no struct is chosen",
      MIXED_STRUCTS,
      null,
      "QuoteCatalogue",
    ],
    [
      "preserves an explicitly selected nested struct when present",
      CATALOGUE_STRUCTS,
      "QuoteAuthor",
      "QuoteAuthor",
    ],
    [
      "preserves an explicitly selected REST struct when present",
      MIXED_STRUCTS,
      "QuotesApi",
      "QuotesApi",
    ],
    [
      "falls back to the primary non-REST struct if the previous selection was removed",
      CATALOGUE_STRUCTS,
      "DeletedStruct",
      "QuoteCatalogue",
    ],
    [
      "falls back to the REST struct when the schema only declares REST structs",
      REST_ONLY_STRUCTS,
      null,
      "UserApi",
    ],
    [
      "returns empty string when no structs are declared",
      [] as StructInfo[],
      "QuoteCatalogue",
      "",
    ],
  ])("%s", (_label, structs, selected, expected) => {
    expect(selectDefaultStructName(structs, selected)).toBe(expected);
  });
});

describe("getHtmlTestActionState", () => {
  it("enables Run Parser and Check Health for HTML and Raw structs when worker is ready", () => {
    const state = getHtmlTestActionState(
      CATALOGUE_STRUCTS,
      null,
      true,
      false,
      false,
    );
    expect(state.activeStruct?.name).toBe("QuoteCatalogue");
    expect(state.isRestSelected).toBe(false);
    expect(state.canRun).toBe(true);
    expect(state.restNotice).toBeNull();
  });

  it("disables Run Parser and Check Health with an explanatory notice when a (rest)struct is selected", () => {
    const state = getHtmlTestActionState(
      MIXED_STRUCTS,
      "QuotesApi",
      true,
      false,
      false,
    );
    expect(state.activeStruct?.name).toBe("QuotesApi");
    expect(state.isRestSelected).toBe(true);
    expect(state.canRun).toBe(false);
    expect(state.restNotice).toContain("REST schema: lint & codegen only");
    expect(state.restNotice).toContain("QuotesApi");
  });

  it("disables actions when worker is loading, schema has errors, or a test is running", () => {
    expect(
      getHtmlTestActionState(CATALOGUE_STRUCTS, null, false, false, false)
        .canRun,
    ).toBe(false);
    expect(
      getHtmlTestActionState(CATALOGUE_STRUCTS, null, true, true, false).canRun,
    ).toBe(false);
    expect(
      getHtmlTestActionState(CATALOGUE_STRUCTS, null, true, false, true).canRun,
    ).toBe(false);
    expect(getHtmlTestActionState([], null, true, false, false).canRun).toBe(
      false,
    );
  });
});

describe("summarizeHealthReport", () => {
  it("counts ok, warn, and fail selector checks accurately", () => {
    const report: StructHealthReport = {
      struct_name: "ProductCard",
      passed: false,
      checks: [
        {
          path: "ProductCard.title",
          selector_type: "css",
          query: "h1.title",
          matches: 1,
          status: "ok",
          message: "(1 matches)",
        },
        {
          path: "ProductCard.discount",
          selector_type: "css",
          query: ".discount",
          matches: 0,
          status: "warn",
          message: "(0 matches, fallback='0%')",
        },
        {
          path: "ProductCard.badges",
          selector_type: "css-all",
          query: ".missing-badge",
          matches: 0,
          status: "fail",
          message: "(0 matches)",
        },
      ],
    };

    expect(summarizeHealthReport(report)).toEqual({
      total: 3,
      okCount: 1,
      warnCount: 1,
      failCount: 1,
      passed: false,
    });
  });

  it("returns zero counts when health report is null", () => {
    expect(summarizeHealthReport(null)).toEqual({
      total: 0,
      okCount: 0,
      warnCount: 0,
      failCount: 0,
      passed: false,
    });
  });
});

describe("testHtml runtime & selector health execution", () => {
  it("executes quotes-catalogue preset including nested QuoteAuthor selectors", async () => {
    const preset = getPresetById("quotes-catalogue")!;
    const res = await testHtml(preset.kdl, preset.html);
    expect(res.ok).toBe(true);
    expect(res.struct_name).toBe("QuoteCatalogue");
    expect(res.run_error).toBeNull();
    expect(Array.isArray(res.run_result)).toBe(true);
    const items = res.run_result as Array<Record<string, unknown>>;
    expect(items).toHaveLength(2);
    expect(items[0].quoteText).toBe(
      "Simplicity is prerequisite for reliability.",
    );
    expect(items[0].likes).toBe(142);
    expect(items[0].authorInfo).toEqual({
      name: "Edsger W. Dijkstra",
      profileUrl: "https://example.org/authors/dijkstra",
      tags: ["computer-science", "architecture"],
    });
    expect(res.health?.passed).toBe(true);
    expect(
      res.health?.checks.some(
        (c) => c.path === "QuoteCatalogue.author_info[QuoteAuthor].name",
      ),
    ).toBe(true);

    // Also test explicit nested struct selection
    const nestedRes = await testHtml(preset.kdl, preset.html, "QuoteAuthor");
    expect(nestedRes.ok).toBe(true);
    expect(nestedRes.struct_name).toBe("QuoteAuthor");
  });

  it("executes product-detail, specs-table, meta-dictionary, and release-log-raw presets", async () => {
    const product = getPresetById("product-detail")!;
    const prodRes = await testHtml(product.kdl, product.html);
    expect(prodRes.ok).toBe(true);
    expect(prodRes.struct_name).toBe("ProductPage");
    expect(prodRes.health?.passed).toBe(true);
    expect(prodRes.run_result).toEqual({
      title: "Mechanical Split Keyboard Pro",
      priceUsd: 189.5,
      galleryImages: [
        "https://cdn.example.com/kb-front.jpg",
        "https://cdn.example.com/kb-side.jpg",
      ],
      offer: {
        sku: "KB-SPLIT-80",
        currency: "USD",
        in_stock: true,
      },
    });

    const table = getPresetById("specs-table")!;
    const tableRes = await testHtml(table.kdl, table.html);
    expect(tableRes.ok).toBe(true);
    expect(tableRes.struct_name).toBe("HardwareSpecs");
    expect(tableRes.health?.passed).toBe(true);
    expect(tableRes.run_result).toEqual({
      modelName: "KeyChrono Q12 Custom",
      switchType: "Tactile Panda 62g",
      pollingHz: 1000,
      weightKg: 1.45,
      connectivity: ["USB-C", "Bluetooth 5.1", "2.4GHz Wireless"],
    });

    const dict = getPresetById("meta-dictionary")!;
    const dictRes = await testHtml(dict.kdl, dict.html);
    expect(dictRes.ok).toBe(true);
    expect(dictRes.struct_name).toBe("PageMetaMap");
    expect(dictRes.run_result).toEqual({
      author: "Ada Lovelace",
      description: "Declarative selector and REST schema compiler",
      keywords: "kdl, scraping, codegen, rest",
      "theme-color": "#3b82f6",
    });

    const flatRes = await testHtml(dict.kdl, dict.html, "DocLinks");
    expect(flatRes.ok).toBe(true);
    expect(flatRes.struct_name).toBe("DocLinks");
    expect(flatRes.run_result).toEqual([
      "/docs/guide",
      "/docs/syntax",
      "https://github.com/vypivshiy/parsmith",
    ]);

    const raw = getPresetById("release-log-raw")!;
    const rawRes = await testHtml(raw.kdl, raw.html);
    expect(rawRes.ok).toBe(true);
    expect(rawRes.struct_name).toBe("ReleaseLogEntries");
    expect(rawRes.health?.passed).toBe(true);
    expect(rawRes.health?.checks).toHaveLength(1);
    expect(rawRes.health?.checks[0].selector_type).toBe("raw");
    expect(rawRes.health?.checks[0].status).toBe("warn");
  });

  it("reports ok, warn (fallback & remove), and fail selector health statuses and evaluates XPath", async () => {
    const kdl = `
(item)struct CardCheck {
    cleaned {
        css-remove ".ad-banner"
        xpath "//h1[@class='title']"
        text
        trim
    }
    optional_sub {
        css ".missing-subtitle"
        text
        fallback "No subtitle"
    }
    required_missing {
        css ".does-not-exist"
        text
    }
}
`;
    const html = `<div><h1 class="title">Hello XPath</h1></div>`;
    const res = await testHtml(kdl, html);
    expect(res.struct_name).toBe("CardCheck");
    expect(res.health).not.toBeNull();
    expect(res.health?.passed).toBe(false);

    const checks = res.health!.checks;
    expect(checks).toHaveLength(4);
    expect(checks[0]).toMatchObject({
      path: "CardCheck.cleaned",
      selector_type: "css-remove",
      matches: 0,
      status: "warn",
    });
    expect(checks[1]).toMatchObject({
      path: "CardCheck.cleaned",
      selector_type: "xpath",
      query: "//h1[@class='title']",
      matches: 1,
      status: "ok",
    });
    expect(checks[2]).toMatchObject({
      path: "CardCheck.optional_sub",
      selector_type: "css",
      matches: 0,
      status: "warn",
      message: '(0 matches, fallback="No subtitle")',
    });
    expect(checks[3]).toMatchObject({
      path: "CardCheck.required_missing",
      selector_type: "css",
      matches: 0,
      status: "fail",
      message: "(0 matches)",
    });
  });

  it("rejects (rest)struct schemas in testHtml with a clear error", async () => {
    const restPreset = getPresetById("github-rest-api")!;
    const res = await testHtml(restPreset.kdl, restPreset.html);
    expect(res.ok).toBe(false);
    expect(res.struct_name).toBe("RepositoryApi");
    expect(res.run_result).toBeNull();
    expect(res.health).toBeNull();
    expect(res.run_error).toContain("(rest)struct");
  });
});
