import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from 'react';
import WaveSurfer from 'wavesurfer.js';
import { Pause, Play, RotateCcw, RotateCw } from 'lucide-react';
import { api } from '../lib/api';
import type { MediaItem } from '../lib/types';

export interface PlayerHandle { seek(seconds: number, play?: boolean): void }
const fmt = (s: number) => `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}`;

/** Audio: wavesurfer.js waveform. Video: native element. Photo: image. */
export const MediaPlayer = forwardRef<PlayerHandle, { item: MediaItem; onTime(t: number): void }>(function MediaPlayer({ item, onTime }, ref) {
  const box = useRef<HTMLDivElement>(null);
  const ws = useRef<WaveSurfer | null>(null);
  const video = useRef<HTMLVideoElement>(null);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [dur, setDur] = useState(item.duration ?? 0);
  const [rate, setRate] = useState(1);
  const [err, setErr] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const url = api.mediaUrl(item.id);

  useImperativeHandle(ref, () => ({
    seek(s, play = true) {
      if (item.kind === 'audio' && ws.current) { ws.current.setTime(s); if (play) ws.current.play(); }
      else if (video.current) { video.current.currentTime = s; if (play) video.current.play(); }
    },
  }));

  useEffect(() => {
    if (item.kind !== 'audio' || !box.current) return;
    setErr(null); setReady(false); setTime(0);
    const css = getComputedStyle(document.documentElement);
    const w = WaveSurfer.create({
      container: box.current, url, height: 84, barWidth: 2, barGap: 1.5, barRadius: 2, cursorWidth: 2,
      waveColor: css.getPropertyValue('--wave').trim() || '#9aa4b2', progressColor: css.getPropertyValue('--accent').trim() || '#5b6cff',
      cursorColor: css.getPropertyValue('--accent').trim() || '#5b6cff', normalize: true,
    });
    ws.current = w;
    w.on('ready', (d) => { setDur(d); setReady(true); });
    w.on('timeupdate', (t) => { setTime(t); onTime(t); });
    w.on('play', () => setPlaying(true));
    w.on('pause', () => setPlaying(false));
    w.on('finish', () => setPlaying(false));
    w.on('error', (e) => setErr(String(e)));
    return () => { w.destroy(); ws.current = null; setPlaying(false); };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item.id]);

  useEffect(() => { ws.current?.setPlaybackRate(rate); if (video.current) video.current.playbackRate = rate; }, [rate]);

  if (item.kind === 'photo') return <div className="player photo"><img src={url} alt={item.title} /></div>;
  if (item.kind === 'video') {
    return (
      <div className="player video">
        <video ref={video} src={url} controls playsInline onTimeUpdate={(e) => onTime((e.target as HTMLVideoElement).currentTime)} />
      </div>
    );
  }
  const toggle = () => ws.current?.playPause();
  const skip = (d: number) => ws.current && ws.current.setTime(Math.max(0, Math.min(dur, ws.current.getCurrentTime() + d)));
  return (
    <div className="player audio" data-testid="audio-player" data-ready={ready} data-playing={playing}>
      <div className="wave" ref={box} data-testid="waveform" />
      {err && <p className="error">Could not load audio: {err}</p>}
      <div className="controls">
        <button onClick={() => skip(-10)} title="Back 10 s"><RotateCcw size={16} /></button>
        <button className="primary round" onClick={toggle} data-testid="play" aria-label={playing ? 'Pause' : 'Play'}>{playing ? <Pause size={18} /> : <Play size={18} />}</button>
        <button onClick={() => skip(10)} title="Forward 10 s"><RotateCw size={16} /></button>
        <span className="clock" data-testid="clock">{fmt(time)} / {fmt(dur)}</span>
        <span className="spacer" />
        <select value={rate} onChange={(e) => setRate(Number(e.target.value))} aria-label="Speed">
          {[0.75, 1, 1.25, 1.5, 2].map((r) => <option key={r} value={r}>{r}×</option>)}
        </select>
      </div>
    </div>
  );
});
