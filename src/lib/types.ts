export type Provider = "gmail" | "outlook";

export type Account = {
  id: string;
  provider: Provider;
  email: string | null;
  displayName: string | null;
  createdAt: number;
};

export type AppInfo = {
  name: string;
  core: string;
  db: string;
  dataDir: string;
};
