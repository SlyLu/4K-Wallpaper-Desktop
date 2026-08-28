export interface MonitorInfo {
  systemMonitorId: string;
  name: string;
  width: number;
  height: number;
  positionX: number;
  positionY: number;
  primary: boolean;
}

export interface AppStatus {
  appDataDirectory: string;
  databasePath: string;
  platform: string;
  schemaVersion: number;
}

export interface MonitorSlice {
  systemMonitorId: string;
  canvasX: number;
  canvasY: number;
  width: number;
  height: number;
}

export interface MonitorLayout {
  layoutHash: string;
  originX: number;
  originY: number;
  width: number;
  height: number;
  slices: MonitorSlice[];
}
