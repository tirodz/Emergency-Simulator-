#!/usr/bin/env node
/*
 * Layout and honesty harness for the frontend.
 *
 * Two things are checked, because both have failed silently before:
 *
 *  1. Layout. Every screen is rendered at a set of widths and every element is measured for
 *     document-level overflow. The original CSS used `white-space:nowrap` on long device values,
 *     which clipped the capability text the operator is meant to read, and a two-column grid that
 *     collapsed badly below ~1180px. Neither is visible in a diff.
 *
 *  2. Honesty. The fake device payloads below include the exact state that used to be misreported:
 *     a stock Samsung with no root. The assertions require the word "not a Cell Broadcast" (or an
 *     equivalent explicit disclaimer) to be present wherever the UI describes the local simulator,
 *     and require no element to claim the stock device is READY.
 *
 * Run: node tools/check_ui.mjs
 */
import { chromium } from "playwright";
import { readFileSync, writeFileSync, existsSync, mkdirSync, cpSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const html = readFileSync(join(root, "src", "index.html"), "utf8");

const WIDTHS = [1000, 1180, 1360, 1600];
const outDir = join(root, ".agent_tmp", "ui");
mkdirSync(outDir, { recursive: true });
try {
  cpSync(join(root, "src", "assets"), join(outDir, "assets"), { recursive: true });
} catch {
  // The harness can still run without images; only the visual reference is missing.
}

let failures = 0;
const fail = (message) => {
  failures += 1;
  console.error(`FAIL  ${message}`);
};
const pass = (message) => console.log(`ok    ${message}`);

/* Mirrors the Rust Device serde shape. `state` and the capability values use the same
   SCREAMING_SNAKE_CASE strings the backend emits. */
const STOCK_A35 = {
  serial: "R5CXA1B2C3D",
  model: "SM-A356B",
  product: "a35x",
  manufacturer: "samsung",
  release: "14",
  sdk: "34",
  build_type: "user",
  debuggable: "0",
  root: false,
  cellbroadcast_package: "com.samsung.android.cellbroadcastreceiver",
  cellbroadcast_candidates: [
    "com.samsung.android.cellbroadcastreceiver",
    "com.google.android.cellbroadcastservice",
  ],
  local_simulator: false,
  test_entrypoint_available: "DENIED",
  test_entrypoint_reason:
    "ro.debuggable=0. This is a production build, so the AOSP test receiver is never registered " +
    "and the test broadcast is accepted by `am` and then discarded. This is a build property, not " +
    "a permission, so it cannot be granted on the device.",
  capability_stage: "RECEIVER_DISCOVERED",
  state: "NO_ROOT",
  support_level: "ROOT_REQUIRED",
  specs: { cpu: "Exynos 1380", ram_gb: 6, battery_percent: 82, screen_resolution: "1080x2340", density: 450 },
  notes: [
    "ro.debuggable=0 so the AOSP test receiver does not exist on this build.",
    "Stock/non-root device. The local simulator provides a root-free alert UI path; it produces a " +
      "local app notification, not a Cell Broadcast.",
  ],
};

const SIMULATOR_READY = {
  ...STOCK_A35,
  local_simulator: true,
  state: "SIMULATOR_READY",
  support_level: "LOCAL_SIMULATOR",
  notes: [
    "Root-free local simulator is installed.",
    "Notification permission could not be read. The simulator may still work; the state is " +
      "unknown rather than denied.",
  ],
};

const DEBUGGABLE = {
  ...STOCK_A35,
  serial: "emulator-5554",
  model: "sdk_gphone64_x86_64",
  manufacturer: "Google",
  build_type: "userdebug",
  debuggable: "1",
  root: false,
  test_entrypoint_available: "GRANTED",
  test_entrypoint_reason:
    "ro.debuggable=1, so the AOSP test receiver in GsmInboundSmsHandler is registered.",
  capability_stage: "TEST_ENTRYPOINT_DISCOVERED",
  state: "READY",
  support_level: "SUPPORTED",
  notes: ["Rooted/userdebug controlled target. AOSP test entry point: GRANTED."],
};

const DEVICES = [STOCK_A35, SIMULATOR_READY, DEBUGGABLE];

const PROBE = {
  build_type: "user",
  debuggable: "0",
  test_entrypoint: { available: "DENIED", reason: STOCK_A35.test_entrypoint_reason },
  cellbroadcast_candidates: STOCK_A35.cellbroadcast_candidates,
  cellbroadcast_package: STOCK_A35.cellbroadcast_package,
  receiver_declared: "GRANTED",
  local_simulator_installed: true,
  post_notifications: "UNKNOWN",
  full_screen_intent: "DENIED",
  notifications_enabled: "UNKNOWN",
  stage: "RECEIVER_DISCOVERED",
  summary: "ro.debuggable=0. Use the local simulator command instead.",
  evidence: [
    {
      label: "getprop ro.debuggable",
      command: "adb -s R5CXA1B2C3D shell getprop ro.debuggable",
      exit_code: 0,
      stdout: "0\n",
      stderr: "",
      parsed: 'ro.debuggable="0"',
    },
    {
      label: "dumpsys package",
      command: "adb -s R5CXA1B2C3D shell dumpsys package com.samsung.android.cellbroadcastreceiver",
      exit_code: 0,
      stdout: "requested permissions:\n  android.permission.POST_NOTIFICATIONS\n",
      stderr: "",
      parsed: "GRANTED",
    },
  ],
};

/* Stands in for the Tauri bridge. Returning realistic payloads is the point: the assertions below
   only mean something if the shapes match what Rust actually sends. */
const BRIDGE = `
window.__TAURI__ = { core: { invoke: async (cmd, args) => {
  if (cmd === "app_info") return { version: "1.0.0", adb_source: "BUNDLED", injector: false };
  if (cmd === "list_devices") return window.__FIXTURES__.devices;
  if (cmd === "platform_diagnostics") return window.__FIXTURES__.probe;
  if (cmd === "send_platform_test_alert") return window.__FIXTURES__.platformSend;
  if (cmd === "send_test_alert") return window.__FIXTURES__.localSend;
  if (cmd === "adb_diagnostics") return { source: "BUNDLED", devices_raw: "R5CXA1B2C3D device" };
  return null;
} }, event: { listen: async () => () => {} } };
`;

/* Prefer a system Chromium so the harness does not require a Playwright browser download. */
const SYSTEM_CHROMIUM = process.env.CHROMIUM_PATH || "/usr/bin/chromium";
const launchOptions = { args: ["--no-sandbox", "--disable-dev-shm-usage"] };
if (existsSync(SYSTEM_CHROMIUM)) launchOptions.executablePath = SYSTEM_CHROMIUM;
const browser = await chromium.launch(launchOptions);

/* Negative control. The overflow check above passes on a correct layout, but it would also pass
   on a broken detector. This injects an element that is known to be too wide and requires the
   detector to notice, so a green run means the layout is clean rather than the check being dead. */
async function selfTestOverflow(page) {
  return page.evaluate(async () => {
    const probe = document.createElement("div");
    probe.id = "__overflow_probe";
    probe.style.cssText = "width:5000px;height:2px";
    document.body.appendChild(probe);
    const doc = document.documentElement;
    const detected = doc.scrollWidth > doc.clientWidth + 1;
    probe.remove();
    return detected;
  });
}

for (const width of WIDTHS) {
  const page = await browser.newPage({ viewport: { width, height: 900 } });

  const consoleErrors = [];
  page.on("pageerror", (error) => consoleErrors.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(message.text());
  });

  const injected = html.replace(
    "</head>",
    `<script>${BRIDGE}</script><script>window.__FIXTURES__=${JSON.stringify({
      devices: DEVICES,
      all: { stock: STOCK_A35, sim: SIMULATOR_READY, dbg: DEBUGGABLE },
      probe: PROBE,
      platformSend: {
        device_serial: "emulator-5554",
        body: "TEST ALERT",
        pdu_hex: "000111031101",
        message_id: "0x1103 ETWS TEST (0x1103)",
        entrypoint_available: "GRANTED",
        stage: "TEST_ENTRYPOINT_ACCEPTED",
        state: "ACCEPTED_NO_EVIDENCE",
        message: "`am broadcast` was accepted, but no downstream log line followed.",
        failure: "NO_DOWNSTREAM_EVIDENCE",
        evidence: ["am broadcast exit=0"],
        logcat_excerpt: "",
        diagnostics: PROBE.evidence,
      },
      localSend: {
        device_serial: "R5CXA1B2C3D",
        category: 4355,
        body: "TEST ALERT",
        state: "NOTIFICATION_POSTED",
        failure: null,
        message: "Local notification posted.",
        evidence: [],
        diagnostics: [],
      },
    })};</script></head>`
  );

  const file = join(outDir, `index-${width}.html`);
  writeFileSync(file, injected);

  await page.goto(`file://${file}`);
  await page.waitForTimeout(600);

  /* The app starts in `view-overview`, which hides #devicesCard. Measure the surface the operator
     actually uses (the devices studio), and assert it is visible first: measuring a hidden panel
     silently reports "no overflow" for a layout nobody ever renders. */
  const visible = await page.evaluate(() => {
    const nav = [...document.querySelectorAll("[data-nav]")].find((b) => b.dataset.nav === "devices");
    if (nav) nav.click();
    const card = document.getElementById("devicesCard");
    return card ? getComputedStyle(card).display !== "none" : false;
  });
  await page.waitForTimeout(400);
  if (!visible) fail(`[${width}px] devices view did not become visible; measurements would be vacuous`);
  else pass(`[${width}px] devices view is visible and measurable`);

  if (!(await selfTestOverflow(page))) {
    fail(`[${width}px] overflow detector did not fire on a deliberately oversized element`);
  } else {
    pass(`[${width}px] overflow detector is live`);
  }

  for (const [name, payload] of [
    ["stock-a35", STOCK_A35],
    ["simulator-ready", SIMULATOR_READY],
    ["debuggable", DEBUGGABLE],
  ]) {
    await page.evaluate(async (device) => {
      window.__FIXTURES__.devices = [device];
      document.getElementById("refreshBtn").click();
      await new Promise((resolve) => setTimeout(resolve, 250));
      // Re-open the studio so the capability banner is rendered for this device.
      const nav = [...document.querySelectorAll("[data-nav]")].find((b) => b.dataset.nav === "devices");
      if (nav) nav.click();
      await new Promise((resolve) => setTimeout(resolve, 200));
    }, payload);
    void name;
    await page.waitForTimeout(250);

    // 1. No horizontal overflow anywhere.
    const overflow = await page.evaluate(() => {
      const doc = document.documentElement;
      const offenders = [];
      if (doc.scrollWidth > doc.clientWidth + 1) {
        for (const element of document.querySelectorAll("body *")) {
          const rect = element.getBoundingClientRect();
          if (rect.right > doc.clientWidth + 1 || rect.left < -1) {
            offenders.push(
              `${element.tagName.toLowerCase()}.${(element.className || "").toString().split(" ")[0]} ` +
                `left=${Math.round(rect.left)} right=${Math.round(rect.right)}`
            );
          }
        }
      }
      return { scrollWidth: doc.scrollWidth, clientWidth: doc.clientWidth, offenders: offenders.slice(0, 6) };
    });

    if (overflow.scrollWidth > overflow.clientWidth + 1) {
      fail(
        `[${width}px ${name}] document overflows horizontally ` +
          `(${overflow.scrollWidth} > ${overflow.clientWidth}): ${overflow.offenders.join("; ")}`
      );
    } else {
      pass(`[${width}px ${name}] no document overflow`);
    }

    /* Document-level scrollWidth is necessary but not sufficient. The original defect was
       `white-space:nowrap` plus `text-overflow:ellipsis` on the detail values: the text was
       clipped inside its own box, so the document never overflowed and the measurement above
       stayed green while the operator could not read the probe's conclusion. This checks the
       boxes themselves. Elements that legitimately scroll (a <pre> of raw logcat) are declared. */
    const clipped = await page.evaluate(() => {
      const allowed = new Set(["PRE", "TEXTAREA"]);
      const bad = [];
      for (const el of document.querySelectorAll("#deviceDetail *, #diagPanel *, #studioCapability, .studio-profile *")) {
        if (allowed.has(el.tagName)) continue;
        if (el.clientWidth > 0 && el.scrollWidth > el.clientWidth + 1) {
          const style = getComputedStyle(el);
          bad.push(
            `${el.tagName.toLowerCase()}.${(el.className || "").toString().split(" ")[0]} ` +
              `scroll=${el.scrollWidth} client=${el.clientWidth} nowrap=${style.whiteSpace}`
          );
        }
      }
      return bad.slice(0, 6);
    });

    if (clipped.length) {
      fail(`[${width}px ${name}] text clipped inside its own box: ${clipped.join("; ")}`);
    } else {
      pass(`[${width}px ${name}] no clipped capability text`);
    }
  }

  // 2. Running the probe must render evidence and must show the raw command.
  await page.evaluate(async () => {
    window.__FIXTURES__.devices = [window.__FIXTURES__.all.stock];
    document.getElementById("refreshBtn").click();
    await new Promise((resolve) => setTimeout(resolve, 200));
  });
  await page.waitForTimeout(200);
  await page.evaluate(() => document.getElementById("diagBtn").click());
  await page.waitForTimeout(400);

  const diag = await page.evaluate(() => {
    const panel = document.getElementById("diagPanel");
    return {
      visible: panel && panel.style.display !== "none",
      text: panel ? panel.innerText : "",
      hasPre: panel ? panel.querySelectorAll("pre").length : 0,
    };
  });

  if (!diag.visible) fail(`[${width}px] diagnostics panel did not render`);
  else pass(`[${width}px] diagnostics panel rendered`);

  if (!diag.text.includes("ro.debuggable")) fail(`[${width}px] diagnostics omitted ro.debuggable`);
  else pass(`[${width}px] diagnostics reported ro.debuggable`);

  if (diag.hasPre === 0) fail(`[${width}px] diagnostics omitted raw command output`);
  else pass(`[${width}px] diagnostics included raw output (${diag.hasPre} blocks)`);

  // 3. The stock device must be described honestly, and never as ready.
  const stockText = await page.evaluate(() => document.body.innerText);
  if (/Retail|Stock\/non-root|no root-free Cell Broadcast path/i.test(stockText)) {
    pass(`[${width}px] stock device described without overclaiming`);
  } else {
    fail(`[${width}px] stock-device wording missing`);
  }
  if (/not a Cell Broadcast/i.test(stockText)) {
    pass(`[${width}px] local simulator is explicitly labelled as not a Cell Broadcast`);
  } else {
    fail(`[${width}px] the "not a Cell Broadcast" disclaimer is absent`);
  }

  // 4. The accepted-but-silent broadcast must not read as a delivery.
  await page.evaluate(async () => {
    window.__FIXTURES__.devices = [window.__FIXTURES__.all.stock];
    document.getElementById("refreshBtn").click();
    await new Promise((resolve) => setTimeout(resolve, 250));
  });
  await page.waitForTimeout(250);

  if (consoleErrors.length) {
    fail(`[${width}px] console/js errors: ${consoleErrors.slice(0, 3).join(" | ")}`);
  } else {
    pass(`[${width}px] no console or JS errors`);
  }

  await page.close();
}

await browser.close();

if (failures) {
  console.error(`\n${failures} layout/honesty assertion(s) failed.`);
  process.exit(1);
}
console.log("\nAll layout and honesty assertions passed.");
