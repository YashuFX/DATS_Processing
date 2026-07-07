import { useState, useEffect, useRef } from "react";

export interface FileDetails {
  path: string;
  size_bytes: number;
  estimated_packets: number;
  estimated_duration_seconds: number;
  file_type: string;
}

export interface RecentPacket {
  sequence_number: number;
  timestamp_ns: number;
  apid: number | null;
}

export interface PlaybackStatus {
  state: string;
  speed: number;
  loop_enabled: boolean;
  packets_published: number;
  total_packets_estimated: number;
  progress_percent: number;
  current_timestamp: number;
}

const BASE_URL = "http://localhost:8080/api/v1/replay";

export function useSimulatorApi() {
  const [state, setState] = useState<string>("STOPPED");
  const [speed, setSpeed] = useState<number>(1.0);
  const [loopEnabled, setLoopEnabled] = useState<boolean>(false);
  const [packetsPublished, setPacketsPublished] = useState<number>(0);
  const [totalPacketsEstimated, setTotalPacketsEstimated] = useState<number>(0);
  const [progressPercent, setProgressPercent] = useState<number>(0);
  const [currentTimestamp, setCurrentTimestamp] = useState<number>(0);
  const [fileDetails, setFileDetails] = useState<FileDetails | null>(null);
  const [recentPackets, setRecentPackets] = useState<RecentPacket[]>([]);
  
  const [serverOnline, setServerOnline] = useState<boolean>(false);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  
  const pollingRef = useRef<number | null>(null);

  // Check server health
  const checkHealth = async () => {
    try {
      const res = await fetch("http://localhost:8080/health/live");
      if (res.ok) {
        setServerOnline(true);
      } else {
        setServerOnline(false);
      }
    } catch {
      setServerOnline(false);
    }
  };

  // Fetch full status
  const fetchStatus = async () => {
    try {
      const res = await fetch(`${BASE_URL}/status`);
      if (!res.ok) throw new Error("Failed to fetch status");
      const data = await res.json();
      
      setState(data.state);
      setSpeed(data.playback.speed);
      setLoopEnabled(data.playback.loop_enabled);
      setPacketsPublished(data.progress.packets_published);
      setTotalPacketsEstimated(data.progress.total_packets_estimated);
      setProgressPercent(data.progress.progress_percent);
      setCurrentTimestamp(data.progress.current_timestamp);
      setErrorMsg(null);
      setServerOnline(true);
      
      // If we don't have file details but we are loaded, populate some dummy details
      if (!fileDetails && data.state !== "STOPPED") {
        setFileDetails({
          path: "Remote File",
          size_bytes: 0,
          estimated_packets: data.progress.total_packets_estimated,
          estimated_duration_seconds: 0,
          file_type: "unknown"
        });
      }
    } catch (e: any) {
      setErrorMsg(e.message || "Failed to communicate with simulator engine");
    }
  };

  // Fetch recent packets
  const fetchPackets = async () => {
    try {
      const res = await fetch(`${BASE_URL}/packets`);
      if (!res.ok) throw new Error("Failed to fetch recent packets");
      const data = await res.json();
      setRecentPackets(data);
    } catch (e: any) {
      console.error(e.message);
    }
  };

  // Load File
  const loadFile = async (filePath: string, fileType: string, targetStage: number = 1) => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/load`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          file_path: filePath,
          file_type: fileType,
          target_stage: targetStage
        })
      });
      const data = await res.json();
      if (!res.ok) {
        throw new Error(data.message || "Load failed");
      }
      setFileDetails(data.file);
      setState(data.status);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
      throw e;
    }
  };

  // Start Playback
  const startPlayback = async (playbackSpeed: number = 1.0, loop: boolean = false) => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/start`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          speed: playbackSpeed,
          loop_enabled: loop
        })
      });
      const data = await res.json();
      if (!res.ok) {
        throw new Error(data.message || "Start failed");
      }
      setState(data.status);
      setSpeed(data.speed);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
      throw e;
    }
  };

  // Pause
  const pausePlayback = async () => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/pause`, { method: "POST" });
      const data = await res.json();
      if (!res.ok) throw new Error(data.message || "Pause failed");
      setState(data.status);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
    }
  };

  // Resume
  const resumePlayback = async () => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/resume`, { method: "POST" });
      const data = await res.json();
      if (!res.ok) throw new Error(data.message || "Resume failed");
      setState(data.status);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
    }
  };

  // Stop
  const stopPlayback = async () => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/stop`, { method: "POST" });
      const data = await res.json();
      if (!res.ok) throw new Error(data.message || "Stop failed");
      setState(data.status);
      setFileDetails(null);
      setRecentPackets([]);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
    }
  };

  // Seek
  const seekPlayback = async (targetTimestamp: number) => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/seek`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ target_timestamp: targetTimestamp })
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.message || "Seek failed");
      setState(data.status);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
    }
  };

  // Set Speed
  const changeSpeed = async (newSpeed: number) => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/speed`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ speed: newSpeed })
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.message || "Speed adjustment failed");
      setSpeed(data.speed);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
    }
  };

  // Set Loop
  const changeLoop = async (enabled: boolean) => {
    setErrorMsg(null);
    try {
      const res = await fetch(`${BASE_URL}/loop`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ enabled })
      });
      const data = await res.json();
      if (!res.ok) throw new Error(data.message || "Loop adjustment failed");
      setLoopEnabled(data.loop_enabled);
      await fetchStatus();
    } catch (e: any) {
      setErrorMsg(e.message);
    }
  };

  // Initial checks and polling setup
  useEffect(() => {
    checkHealth();
    fetchStatus();

    const healthInterval = window.setInterval(checkHealth, 3000);
    return () => clearInterval(healthInterval);
  }, []);

  // Poll status & packets when running or active
  useEffect(() => {
    if (state === "RUNNING") {
      pollingRef.current = window.setInterval(() => {
        fetchStatus();
        fetchPackets();
      }, 250);
    } else {
      if (pollingRef.current !== null) {
        clearInterval(pollingRef.current);
        pollingRef.current = null;
      }
      // Fetch status one last time to capture stable pause/stop state
      fetchStatus();
      fetchPackets();
    }

    return () => {
      if (pollingRef.current !== null) {
        clearInterval(pollingRef.current);
        pollingRef.current = null;
      }
    };
  }, [state]);

  return {
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
    refreshStatus: fetchStatus
  };
}
