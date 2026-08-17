export interface AppConfig {
  input_folder: string;
  output_folder: string;
  override_existed: boolean;
  template_name: string;
  template: string;
  quality: number;
  templates: string[];
}

export interface FileNode {
  label: string;
  value?: string;
  is_file?: boolean;
  children?: FileNode[];
}

export interface FileTrees {
  input_files: FileNode[];
  output_files: FileNode[];
}

export interface ProgressState {
  active: boolean;
  complete: boolean;
  total: number;
  processed: number;
  success: number;
  failure: number;
  skipped: number;
  percent: number;
  current: string;
  message: string;
}

export interface EngineEvent {
  event: "start" | "progress" | "complete" | "error";
  data: Partial<ProgressState> & { message?: string };
}
