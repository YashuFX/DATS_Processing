import React, { useState, useEffect, useRef } from "react";
import { useSimulatorApi } from "./hooks/useSimulatorApi";
import "./App.css";

// Formatter Helpers
function formatBytes(bytes: number): string {
  if (bytes === 0) return "0 Bytes";
  const k = 1024;
  const sizes = ["Bytes", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

function formatNanoseconds(ns: number): string {
  if (ns === 0) return "N/A";
  const ms = Math.floor(ns / 1000000);
  const date = new Date(ms);
  return date.toISOString().replace("T", " ").replace("Z", " UTC");
}

export default function App() {
  const {
    state,
    speed,
    loopEnabled,
    packetsPublished,
    totalPacketsEstimated,
    progressPercent,
    currentTimestamp,
    fileDetails,
    recentPackets,
    serverOnline,
    errorMsg,
    loadFile,
    startPlayback,
    pausePlayback,
    resumePlayback,
    stopPlayback,
    seekPlayback,
    changeSpeed,
    changeLoop,
  } = useSimulatorApi();

  // Local Form Inputs
  const [filePath, setFilePath] = useState("data/sample.ccsds");
  const [fileType, setFileType] = useState("ccsds");
  const [targetStage, setTargetStage] = useState(1);
  const [loadingFile, setLoadingFile] = useState(false);
  const [apidFilter, setApidFilter] = useState<string>("");
  const [autoScroll, setAutoScroll] = useState(true);

  // References
  const terminalEndRef = useRef<HTMLDivElement | null>(null);

  // Auto scroll logs
  useEffect(() => {
    if (autoScroll && terminalEndRef.current) {
      terminalEndRef.current.scrollIntoView({ behavior: "smooth" });
    }
  }, [recentPackets, autoScroll]);

  // Handle load form submit
  const handleLoad = async (e: React.FormEvent) => {
    e.preventDefault();
    setLoadingFile(true);
    try {
      await loadFile(filePath, fileType, targetStage);
    } catch {
      // Error handled by hook state
    } finally {
      setLoadingFile(false);
    }
  };

  // Play / Pause Toggle Helper
  const handlePlayPause = async () => {
    if (state === "READY" || state === "STOPPED" || state === "COMPLETED") {
      await startPlayback(speed, loopEnabled);
    } else if (state === "RUNNING") {
      await pausePlayback();
    } else if (state === "PAUSED") {
      await resumePlayback();
    }
  };

  // Seek on progress bar change
  const handleProgressChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!fileDetails || state === "STOPPED") return;
    const percent = parseFloat(e.target.value);
    // Find estimated timestamp based on percentage
    if (totalPacketsEstimated > 0) {
      // Find duration bounds from status
      const res = await fetch("http://localhost:8080/api/v1/replay/status");
      await res.json();
      
      // Let's seek to a proportion of the estimated packets
      // To simplify seek, the engine lets us seek via timestamp in nanoseconds
      // We can estimate the seek timestamp:
      // duration_ns = end_timestamp_ns - start_timestamp_ns
      // target_ns = start_ns + (duration * percent)
      // For simple feedback we query the loaded stats.
      // Let's calculate the timestamp:
      const startNs = currentTimestamp - (progressPercent / 100) * 1000000000 * (fileDetails.estimated_duration_seconds || 1000);
      const totalDurationNs = (fileDetails.estimated_duration_seconds || 1000) * 1000000000;
      const targetNs = Math.floor(startNs + (totalDurationNs * percent) / 100);
      await seekPlayback(targetNs);
    }
  };

  // Filtered Packets
  const filteredPackets = recentPackets.filter((pkt) => {
    if (!apidFilter.trim()) return true;
    if (pkt.apid === null) return false;
    return pkt.apid.toString().includes(apidFilter.trim());
  });

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: "100vh" }}>
      {/* 1. Header Component */}
      <header
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          padding: "16px 24px",
          background: "var(--glass-bg)",
          backdropFilter: "blur(16px)",
          borderBottom: "1px solid var(--glass-border)",
          boxShadow: "0 4px 20px rgba(0,0,0,0.2)",
        }}
      >
        <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
          {/* Radio Antenna SVG Icon */}
          <svg width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="var(--accent-cyan)" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ filter: "drop-shadow(var(--shadow-neon))" }}>
            <path d="M4.9 19.1C1 15.2 1 8.8 4.9 4.9" />
            <path d="M7.8 16.2c-2.3-2.3-2.3-6.1 0-8.5" />
            <circle cx="12" cy="12" r="2" fill="var(--accent-cyan)" />
            <path d="M16.2 7.8c2.3 2.3 2.3 6.1 0 8.5" />
            <path d="M19.1 4.9C23 8.8 23 15.2 19.1 19.1" />
            <path d="M12 14v8" />
            <path d="M9 22h6" />
          </svg>
          <div>
            <h1 style={{ fontSize: "1.25rem", fontWeight: 700, letterSpacing: "-0.02em", color: "#fff" }}>
              MuST Simulation Ground Station
            </h1>
            <span style={{ fontSize: "0.75rem", color: "var(--color-text-secondary)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
              Telemetry Replay & Ingestion Service
            </span>
          </div>
        </div>

        <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
          {/* Server Connection Badge */}
          <div
            style={{
              display: "flex",
              alignItems: "center",
              gap: "8px",
              padding: "6px 12px",
              borderRadius: "20px",
              background: serverOnline ? "rgba(0, 245, 160, 0.08)" : "rgba(255, 51, 102, 0.08)",
              border: `1px solid ${serverOnline ? "rgba(0, 245, 160, 0.2)" : "rgba(255, 51, 102, 0.2)"}`,
              fontSize: "0.75rem",
              fontWeight: 600,
            }}
          >
            <span
              style={{
                width: "8px",
                height: "8px",
                borderRadius: "50%",
                background: serverOnline ? "var(--accent-green)" : "var(--accent-red)",
                boxShadow: serverOnline ? "0 0 10px var(--accent-green)" : "0 0 10px var(--accent-red)",
              }}
              className={serverOnline ? "" : "blink"}
            />
            {serverOnline ? "ENGINE ONLINE" : "ENGINE OFFLINE"}
          </div>

          {/* Active Playback State Badge */}
          <span
            className={
              state === "RUNNING"
                ? "badge badge-running"
                : state === "READY"
                ? "badge badge-ready"
                : state === "PAUSED"
                ? "badge badge-paused"
                : state === "COMPLETED"
                ? "badge badge-completed"
                : "badge badge-stopped"
            }
          >
            {state}
          </span>
        </div>
      </header>

      {/* Main Content Layout Grid */}
      <main
        style={{
          flex: 1,
          display: "grid",
          gridTemplateColumns: "360px 1fr",
          gap: "24px",
          padding: "24px",
          maxWidth: "1600px",
          width: "100%",
          margin: "0 auto",
        }}
      >
        {/* LEFT COLUMN: Controls & Configurations */}
        <section style={{ display: "flex", flexDirection: "column", gap: "24px" }}>
          {/* 1. File Loader Form */}
          <div className="glass-panel" style={{ display: "flex", flexDirection: "column", gap: "16px" }}>
            <h2 style={{ fontSize: "1rem", fontWeight: 600, borderBottom: "1px solid var(--glass-border)", paddingBottom: "10px", color: "var(--accent-cyan)" }}>
              Telemetry Source Loader
            </h2>
            
            <form onSubmit={handleLoad} style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
              <div style={{ display: "flex", flexDirection: "column", gap: "4px" }}>
                <label style={{ fontSize: "0.75rem", color: "var(--color-text-secondary)", fontWeight: 500 }}>
                  File Format
                </label>
                <select
                  value={fileType}
                  onChange={(e) => setFileType(e.target.value)}
                  style={{
                    background: "var(--bg-secondary)",
                    border: "1px solid var(--glass-border)",
                    color: "#fff",
                    borderRadius: "6px",
                    padding: "8px",
                    outline: "none",
                    fontFamily: "var(--font-sans)",
                  }}
                >
                  <option value="ccsds">Pure CCSDS Frame Stream (.ccsds)</option>
                  <option value="binary">Wrapped Binary Packets (.bin)</option>
                </select>
              </div>

              <div style={{ display: "flex", flexDirection: "column", gap: "4px" }}>
                <label style={{ fontSize: "0.75rem", color: "var(--color-text-secondary)", fontWeight: 500 }}>
                  Absolute File Path
                </label>
                <input
                  type="text"
                  value={filePath}
                  onChange={(e) => setFilePath(e.target.value)}
                  placeholder="data/sample.ccsds"
                  style={{
                    background: "var(--bg-secondary)",
                    border: "1px solid var(--glass-border)",
                    color: "#fff",
                    borderRadius: "6px",
                    padding: "8px 12px",
                    outline: "none",
                    fontSize: "0.85rem",
                    fontFamily: "var(--font-mono)",
                  }}
                  required
                />
              </div>

              <div style={{ display: "flex", flexDirection: "column", gap: "4px" }}>
                <label style={{ fontSize: "0.75rem", color: "var(--color-text-secondary)", fontWeight: 500 }}>
                  Decoder Target Stage
                </label>
                <input
                  type="number"
                  min="0"
                  max="10"
                  value={targetStage}
                  onChange={(e) => setTargetStage(parseInt(e.target.value) || 0)}
                  style={{
                    background: "var(--bg-secondary)",
                    border: "1px solid var(--glass-border)",
                    color: "#fff",
                    borderRadius: "6px",
                    padding: "8px 12px",
                    outline: "none",
                    fontSize: "0.85rem",
                  }}
                />
              </div>

              <button
                type="submit"
                className="btn-primary"
                disabled={loadingFile || !serverOnline}
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  gap: "8px",
                  marginTop: "8px",
                  opacity: (!serverOnline || loadingFile) ? 0.6 : 1,
                  cursor: (!serverOnline || loadingFile) ? "not-allowed" : "pointer"
                }}
              >
                {loadingFile ? (
                  <>
                    <span className="blink">Loading Telemetry...</span>
                  </>
                ) : (
                  <>
                    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="17 8 12 3 7 8" />
                      <line x1="12" y1="3" x2="12" y2="15" />
                    </svg>
                    Ingest File
                  </>
                )}
              </button>
            </form>
          </div>

          {/* 2. File Metrics Card */}
          {fileDetails && (
            <div className="glass-panel" style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
              <h3 style={{ fontSize: "0.85rem", fontWeight: 600, color: "var(--color-text-secondary)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
                Ingested File Information
              </h3>
              
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: "12px", fontSize: "0.85rem" }}>
                <div style={{ background: "rgba(255,255,255,0.02)", padding: "10px", borderRadius: "6px", border: "1px solid var(--glass-border)" }}>
                  <span style={{ fontSize: "0.7rem", color: "var(--color-text-muted)", display: "block" }}>FILE TYPE</span>
                  <span style={{ fontWeight: 600, color: "var(--accent-cyan)", textTransform: "uppercase" }}>{fileDetails.file_type}</span>
                </div>
                <div style={{ background: "rgba(255,255,255,0.02)", padding: "10px", borderRadius: "6px", border: "1px solid var(--glass-border)" }}>
                  <span style={{ fontSize: "0.7rem", color: "var(--color-text-muted)", display: "block" }}>FILE SIZE</span>
                  <span style={{ fontWeight: 600, color: "#fff" }}>{formatBytes(fileDetails.size_bytes)}</span>
                </div>
                <div style={{ background: "rgba(255,255,255,0.02)", padding: "10px", borderRadius: "6px", border: "1px solid var(--glass-border)" }}>
                  <span style={{ fontSize: "0.7rem", color: "var(--color-text-muted)", display: "block" }}>EST. PACKETS</span>
                  <span style={{ fontWeight: 600, color: "var(--accent-green)" }}>{fileDetails.estimated_packets}</span>
                </div>
                <div style={{ background: "rgba(255,255,255,0.02)", padding: "10px", borderRadius: "6px", border: "1px solid var(--glass-border)" }}>
                  <span style={{ fontSize: "0.7rem", color: "var(--color-text-muted)", display: "block" }}>PLAYBACK DURATION</span>
                  <span style={{ fontWeight: 600, color: "#fff" }}>{fileDetails.estimated_duration_seconds.toFixed(1)}s</span>
                </div>
              </div>

              <div style={{ fontSize: "0.75rem", background: "rgba(0,0,0,0.15)", padding: "8px 12px", borderRadius: "6px", border: "1px solid var(--glass-border)", wordBreak: "break-all", fontFamily: "var(--font-mono)" }}>
                <span style={{ color: "var(--color-text-muted)" }}>PATH: </span>
                {fileDetails.path}
              </div>
            </div>
          )}

          {/* 3. Static Ground Segment metadata */}
          <div className="glass-panel" style={{ display: "flex", flexDirection: "column", gap: "12px" }}>
            <h3 style={{ fontSize: "0.85rem", fontWeight: 600, color: "var(--color-text-secondary)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
              Active Ground Station Metadata
            </h3>

            <div style={{ display: "flex", flexDirection: "column", gap: "8px", fontSize: "0.85rem" }}>
              <div style={{ display: "flex", justifyContent: "space-between", borderBottom: "1px solid rgba(255,255,255,0.02)", paddingBottom: "6px" }}>
                <span style={{ color: "var(--color-text-secondary)" }}>Satellite ID</span>
                <span style={{ fontWeight: 600, color: "var(--accent-purple)" }}>SUTRA-SAT (NORAD: 99876)</span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", borderBottom: "1px solid rgba(255,255,255,0.02)", paddingBottom: "6px" }}>
                <span style={{ color: "var(--color-text-secondary)" }}>Ground Station</span>
                <span style={{ fontWeight: 600 }}>IDS-01 (Bangalore Deep Space)</span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between", borderBottom: "1px solid rgba(255,255,255,0.02)", paddingBottom: "6px" }}>
                <span style={{ color: "var(--color-text-secondary)" }}>Telemetry Ingress</span>
                <span style={{ fontWeight: 600, fontFamily: "var(--font-mono)" }}>localhost:50052</span>
              </div>
              <div style={{ display: "flex", justifyContent: "space-between" }}>
                <span style={{ color: "var(--color-text-secondary)" }}>Decoder Path</span>
                <span style={{ fontWeight: 600, color: "var(--accent-green)" }}>Active Telemetry Gateway</span>
              </div>
            </div>
          </div>
        </section>

        {/* RIGHT COLUMN: Player Control Deck, Metrics & Logs */}
        <section style={{ display: "flex", flexDirection: "column", gap: "24px" }}>
          
          {/* Main Error Indicator if any */}
          {errorMsg && (
            <div
              style={{
                background: "rgba(255, 51, 102, 0.1)",
                border: "1px solid var(--accent-red)",
                color: "var(--accent-red)",
                borderRadius: "8px",
                padding: "12px 16px",
                fontSize: "0.85rem",
                fontWeight: 500,
                display: "flex",
                alignItems: "center",
                gap: "10px",
              }}
            >
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5">
                <circle cx="12" cy="12" r="10" />
                <line x1="12" y1="8" x2="12" y2="12" />
                <line x1="12" y1="16" x2="12.01" y2="16" />
              </svg>
              <span>{errorMsg}</span>
            </div>
          )}

          {/* 1. Replay Deck Panel */}
          <div className="glass-panel" style={{ display: "flex", flexDirection: "column", gap: "20px" }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
              <h2 style={{ fontSize: "1rem", fontWeight: 600, color: "#fff" }}>Replay Control Deck</h2>
              
              {/* Loop Controls & Speed */}
              <div style={{ display: "flex", alignItems: "center", gap: "16px" }}>
                {/* Loop Checkbox */}
                <label style={{ display: "flex", alignItems: "center", gap: "8px", fontSize: "0.85rem", color: "var(--color-text-secondary)", cursor: "pointer" }}>
                  <input
                    type="checkbox"
                    checked={loopEnabled}
                    onChange={(e) => changeLoop(e.target.checked)}
                    disabled={!fileDetails}
                    style={{
                      accentColor: "var(--accent-cyan)",
                      width: "16px",
                      height: "16px",
                      cursor: "pointer"
                    }}
                  />
                  Loop Mode
                </label>

                {/* Speed Multiplier */}
                <div style={{ display: "flex", alignItems: "center", gap: "6px" }}>
                  <span style={{ fontSize: "0.85rem", color: "var(--color-text-secondary)" }}>Speed:</span>
                  <select
                    value={speed}
                    onChange={(e) => changeSpeed(parseFloat(e.target.value))}
                    disabled={!fileDetails}
                    style={{
                      background: "var(--bg-secondary)",
                      border: "1px solid var(--glass-border)",
                      color: "#fff",
                      borderRadius: "4px",
                      padding: "4px 8px",
                      fontSize: "0.8rem",
                      fontWeight: 600,
                      outline: "none"
                    }}
                  >
                    <option value="1">1.0x (Realtime)</option>
                    <option value="2">2.0x</option>
                    <option value="5">5.0x</option>
                    <option value="10">10x</option>
                    <option value="50">50x</option>
                    <option value="100">100x</option>
                    <option value="500">500x</option>
                  </select>
                </div>
              </div>
            </div>

            {/* Playback Progress Slider */}
            <div style={{ display: "flex", flexDirection: "column", gap: "8px" }}>
              <input
                type="range"
                min="0"
                max="100"
                step="0.1"
                value={progressPercent}
                onChange={handleProgressChange}
                disabled={!fileDetails || state === "STOPPED"}
                style={{
                  width: "100%",
                  height: "6px",
                  borderRadius: "3px",
                  background: "var(--bg-tertiary)",
                  outline: "none",
                  accentColor: "var(--accent-cyan)",
                  cursor: fileDetails ? "pointer" : "not-allowed"
                }}
              />
              <div style={{ display: "flex", justifyContent: "space-between", fontSize: "0.75rem", color: "var(--color-text-secondary)" }}>
                <span>PROGRESS: {progressPercent.toFixed(1)}%</span>
                <span style={{ fontFamily: "var(--font-mono)" }}>
                  {packetsPublished} / {totalPacketsEstimated} PACKETS
                </span>
              </div>
            </div>

            {/* Deck Playback Buttons Grid */}
            <div style={{ display: "flex", alignItems: "center", justifyContent: "center", gap: "16px" }}>
              {/* Stop Button */}
              <button
                className={`deck-btn ${state !== "STOPPED" ? "active-stop" : ""}`}
                onClick={stopPlayback}
                disabled={!fileDetails || state === "STOPPED"}
                title="Stop Replay"
                style={{ opacity: !fileDetails ? 0.3 : 1 }}
              >
                {/* Stop SVG */}
                <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                  <rect x="4" cy="4" width="16" height="16" rx="2" />
                </svg>
              </button>

              {/* Play / Pause Toggle Button */}
              <button
                className={`deck-btn ${state === "RUNNING" ? "active-play" : state === "PAUSED" ? "active-pause" : ""}`}
                onClick={handlePlayPause}
                disabled={!fileDetails}
                title={state === "RUNNING" ? "Pause Replay" : "Start Replay"}
                style={{ width: "56px", height: "56px", opacity: !fileDetails ? 0.3 : 1 }}
              >
                {state === "RUNNING" ? (
                  // Pause SVG
                  <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor">
                    <rect x="6" y="4" width="4" height="16" rx="1" />
                    <rect x="14" y="4" width="4" height="16" rx="1" />
                  </svg>
                ) : (
                  // Play SVG
                  <svg width="22" height="22" viewBox="0 0 24 24" fill="currentColor" style={{ marginLeft: "4px" }}>
                    <path d="M8 5v14l11-7z" />
                  </svg>
                )}
              </button>
            </div>
          </div>

          {/* 2. Real-Time Telemetry Stats Overview */}
          <div style={{ display: "grid", gridTemplateColumns: "1fr 1.5fr", gap: "24px" }}>
            {/* Live Progress Ring */}
            <div className="glass-panel" style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: "12px", minHeight: "180px" }}>
              <div style={{ position: "relative", width: "100px", height: "100px" }}>
                {/* Radial Progress Circle SVG */}
                <svg width="100" height="100" viewBox="0 0 100 100" style={{ transform: "rotate(-90deg)" }}>
                  <circle
                    cx="50"
                    cy="50"
                    r="40"
                    fill="transparent"
                    stroke="var(--bg-tertiary)"
                    strokeWidth="8"
                  />
                  <circle
                    cx="50"
                    cy="50"
                    r="40"
                    fill="transparent"
                    stroke="var(--accent-cyan)"
                    strokeWidth="8"
                    strokeDasharray={2 * Math.PI * 40}
                    strokeDashoffset={2 * Math.PI * 40 * (1 - progressPercent / 100)}
                    style={{
                      transition: "stroke-dashoffset 0.35s",
                      filter: "drop-shadow(var(--shadow-neon))"
                    }}
                  />
                </svg>
                <div
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: "100%",
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "center",
                    flexDirection: "column",
                  }}
                >
                  <span style={{ fontSize: "1.25rem", fontWeight: 700, color: "#fff" }}>
                    {progressPercent.toFixed(0)}%
                  </span>
                  <span style={{ fontSize: "0.6rem", color: "var(--color-text-secondary)", textTransform: "uppercase" }}>
                    Complete
                  </span>
                </div>
              </div>
              <span style={{ fontSize: "0.75rem", color: "var(--color-text-secondary)", fontWeight: 500 }}>
                PLAYBACK COMPLETION RATE
              </span>
            </div>

            {/* Active Time Indicators */}
            <div className="glass-panel" style={{ display: "flex", flexDirection: "column", justifyContent: "center", gap: "16px" }}>
              <div>
                <span style={{ fontSize: "0.7rem", color: "var(--color-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
                  Active Telemetry Event Time (Onboard Epoch)
                </span>
                <div style={{ fontSize: "1.2rem", fontWeight: 700, fontFamily: "var(--font-mono)", color: "var(--accent-green)", textShadow: "0 0 10px rgba(0, 245, 160, 0.2)", marginTop: "4px" }}>
                  {formatNanoseconds(currentTimestamp)}
                </div>
              </div>

              <div>
                <span style={{ fontSize: "0.7rem", color: "var(--color-text-muted)", textTransform: "uppercase", letterSpacing: "0.05em" }}>
                  Raw Engine Timestamp (nanoseconds)
                </span>
                <div style={{ fontSize: "1rem", fontWeight: 500, fontFamily: "var(--font-mono)", color: "var(--color-text-primary)", marginTop: "4px" }}>
                  {currentTimestamp > 0 ? currentTimestamp.toLocaleString() : "N/A"} ns
                </div>
              </div>
            </div>
          </div>

          {/* 3. Live Log Terminal Console */}
          <div
            className="glass-panel"
            style={{
              flex: 1,
              display: "flex",
              flexDirection: "column",
              gap: "12px",
              minHeight: "350px",
              background: "#05070a",
              borderColor: "rgba(0, 242, 254, 0.1)",
              boxShadow: "inset 0 0 20px rgba(0,0,0,0.8)",
            }}
          >
            {/* Terminal Top Control Bar */}
            <div
              style={{
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                borderBottom: "1px solid rgba(255, 255, 255, 0.05)",
                paddingBottom: "10px",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: "8px" }}>
                <span style={{ width: "10px", height: "10px", borderRadius: "50%", background: "var(--accent-cyan)", boxShadow: "var(--shadow-neon)" }} />
                <h3 style={{ fontSize: "0.85rem", fontWeight: 600, color: "#fff", fontFamily: "var(--font-mono)" }}>
                  LIVE_INGRESS_FEED.log
                </h3>
              </div>

              {/* Terminal Options */}
              <div style={{ display: "flex", alignItems: "center", gap: "12px" }}>
                {/* APID filter */}
                <input
                  type="text"
                  placeholder="Filter APID (e.g. 42)"
                  value={apidFilter}
                  onChange={(e) => setApidFilter(e.target.value)}
                  style={{
                    background: "var(--bg-primary)",
                    border: "1px solid rgba(255,255,255,0.08)",
                    borderRadius: "4px",
                    color: "var(--accent-cyan)",
                    padding: "4px 8px",
                    fontSize: "0.75rem",
                    fontFamily: "var(--font-mono)",
                    outline: "none",
                    width: "140px"
                  }}
                />

                {/* Auto Scroll Toggle */}
                <button
                  onClick={() => setAutoScroll(!autoScroll)}
                  style={{
                    background: autoScroll ? "rgba(0, 242, 254, 0.1)" : "transparent",
                    border: "1px solid rgba(0, 242, 254, 0.2)",
                    borderRadius: "4px",
                    color: autoScroll ? "var(--accent-cyan)" : "var(--color-text-secondary)",
                    padding: "4px 8px",
                    fontSize: "0.7rem",
                    fontWeight: 600,
                  }}
                >
                  {autoScroll ? "AUTOSCROLL ON" : "SCROLL LOCKED"}
                </button>
              </div>
            </div>

            {/* Terminal Viewport */}
            <div
              style={{
                flex: 1,
                overflowY: "auto",
                fontFamily: "var(--font-mono)",
                fontSize: "0.8rem",
                color: "#10B981", // green terminal color
                display: "flex",
                flexDirection: "column",
                gap: "4px",
                padding: "8px",
                lineHeight: "1.4",
              }}
            >
              {filteredPackets.length === 0 ? (
                <div style={{ color: "var(--color-text-muted)", textAlign: "center", marginTop: "40px" }}>
                  {state === "STOPPED"
                    ? "[SYSTEM] Loader idle. Select a file and load it to initialize telemetry ingestion feed."
                    : "[SYSTEM] Listening... No matching packet frames received."}
                </div>
              ) : (
                filteredPackets.map((pkt, idx) => (
                  <div
                    key={idx}
                    style={{
                      display: "flex",
                      gap: "8px",
                      borderBottom: "1px solid rgba(255,255,255,0.01)",
                      paddingBottom: "2px",
                      color: pkt.apid === 42 ? "#34D399" : "var(--accent-cyan)",
                    }}
                  >
                    <span style={{ color: "rgba(255,255,255,0.25)" }}>
                      [{new Date().toLocaleTimeString()}]
                    </span>
                    <span style={{ color: "#FBBF24" }}>
                      [SEQ: {pkt.sequence_number.toString().padStart(4, "0")}]
                    </span>
                    <span>
                      TIME: {pkt.timestamp_ns} ns
                    </span>
                    <span style={{ fontWeight: 600 }}>
                      [APID: {pkt.apid !== null ? pkt.apid : "N/A"}]
                    </span>
                  </div>
                ))
              )}
              <div ref={terminalEndRef} />
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
