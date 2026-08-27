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
}
