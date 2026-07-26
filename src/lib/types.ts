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

export type FolderRole =
  | "inbox"
  | "sent"
  | "drafts"
  | "trash"
  | "junk"
  | "archive"
  | "other";

export type Folder = {
  id: string;
  accountId: string;
  imapName: string;
  delimiter: string | null;
  role: FolderRole;
  name: string;
};

export type MessageFlags = {
  seen: boolean;
  flagged: boolean;
  answered: boolean;
  draft: boolean;
};

export type MessageListItem = {
  id: string;
  accountId: string;
  folderId: string;
  uid: number;
  rfcMessageId: string | null;
  subject: string;
  fromAddress: string | null;
  fromName: string | null;
  toList: string | null;
  dateUnix: number;
  size: number | null;
  flags: MessageFlags;
};

export type MessagePage = {
  messages: MessageListItem[];
  total: number;
  offset: number;
  limit: number;
};

export type FolderSyncProgress = {
  folderId: string;
  fetched: number;
  total: number;
};

export type AppInfo = {
  name: string;
  core: string;
  db: string;
  dataDir: string;
  gmailOauthConfigured: boolean;
  outlookOauthConfigured: boolean;
};
