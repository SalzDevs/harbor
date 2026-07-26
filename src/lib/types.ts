export type Provider = "gmail" | "outlook";

export type AccountStatus = "stub" | "connected";

export type Account = {
  id: string;
  provider: Provider;
  status: AccountStatus;
  email: string | null;
  displayName: string | null;
  createdAt: number;
};

export type AppInfo = {
  name: string;
  core: string;
  db: string;
  dataDir: string;
  gmailOauthConfigured: boolean;
};
