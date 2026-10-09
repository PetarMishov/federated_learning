import { HttpClient } from '@angular/common/http';
import { Injectable, inject, signal } from '@angular/core';
import { tap } from 'rxjs';

export interface Organization {
  id: number;
  name: string;
  owner_user_id: number;
}

export interface OrganizationList {
  organizations: Organization[];
}

export interface Project {
  id: number;
  org_id: number;
  created_by_user_id: number;
  name: string;
}

export interface ProjectList {
  projects: Project[];
}

export interface Deployment {
  id: number;
  org_id: number;
  project_id: number;
  snapshot_id: number;
  name: string;
  status: 'pending' | 'started' | 'finished' | 'cancelled' | 'failed' | 'skipped';
  created_by_user_id: number;
  created_at: number;
  started_at: number | null;
  ended_at: number | null;
}

export interface DeploymentList {
  deployments: Deployment[];
}

export interface Snapshot {
  id: number;
  project_id: number;
  created_by_user_id: number;
  source: 'github' | 'gitlab' | 'local' | 'platform';
  source_branch: string | null;
  source_commit_sha: string | null;
  git_commit_sha: string;
  created_at: number;
}

export interface SnapshotList {
  snapshots: Snapshot[];
  has_more: boolean;
}

export interface SnapshotTreeEntry {
  name: string;
  path: string;
  kind: 'directory' | 'file' | 'symlink' | 'submodule';
}

export interface SnapshotTree {
  path: string;
  entries: SnapshotTreeEntry[];
}

export type SnapshotOperation = { kind: 'delete'; path: string } | { kind: 'move'; path: string; to: string };

export interface SnapshotFile {
  path: string;
  content: string;
}

export interface SnapshotPreviewTooLarge {
  error: 'preview_too_large';
  size_bytes: number;
  max_preview_bytes: number;
}

export interface Member {
  id: number;
  username: string;
  role_id: number | null;
  role_name: string | null;
}

export interface MemberList {
  members: Member[];
}

export interface Notification {
  id: number;
  title: string;
  message?: string;
  is_read: boolean;
  created_at: number;
}

export interface NotificationList {
  notifications: Notification[];
}

export type RepositoryProvider = 'github' | 'gitlab';

export interface Repository {
  id: number;
  full_name: string;
  web_url: string;
}

export interface ProviderConnection {
  provider: RepositoryProvider;
  external_account_id: string;
  external_username: string;
}

export interface RepositoryList {
  repositories: Repository[];
  has_more: boolean;
}

export interface Branch {
  name: string;
  commit_sha: string;
}

export interface BranchList {
  branches: Branch[];
  has_more: boolean;
}

@Injectable({ providedIn: 'root' })
export class UsersApi {
  private readonly http = inject(HttpClient);
  readonly token = signal(sessionStorage.getItem('authToken'));
  readonly username = signal(sessionStorage.getItem('authUsername') ?? 'USERNAME');

  login(username: string, password: string) {
    return this.http.post<string>('/users/verify', { username, password }).pipe(
      tap((token) => {
        sessionStorage.setItem('authToken', token);
        sessionStorage.setItem('authUsername', username);
        this.token.set(token);
        this.username.set(username);
      }),
    );
  }

  logout() {
    return this.http.post<void>('/users/logout', null, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getOrganizations() {
    return this.http.get<OrganizationList>('/users/organizations', {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getNotifications() {
    return this.http.get<NotificationList>('/users/notifications', {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getOrganizationProjects(orgId: string) {
    return this.http.get<ProjectList>(`/organizations/${encodeURIComponent(orgId)}/projects`, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getOrganizationMembers(orgId: string) {
    return this.http.get<MemberList>(`/organizations/${encodeURIComponent(orgId)}/members`, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  authorizeGitlab(token: string) {
    return this.http.post<ProviderConnection>('/connectors/gitlab/authorize', { token }, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  authorizeGithub(token: string) {
    return this.http.post<ProviderConnection>('/connectors/github/authorize', { token }, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getRepositories(provider: RepositoryProvider, page = 1) {
    return this.http.get<RepositoryList>(`/connectors/${provider}/repositories`, {
      headers: { Authorization: `Bearer ${this.token()}` },
      params: { page },
    });
  }

  getBranches(provider: RepositoryProvider, repository: Repository, page = 1) {
    return this.http.get<BranchList>(`/connectors/${provider}/branches`, {
      headers: { Authorization: `Bearer ${this.token()}` },
      params: { repository: provider === 'github' ? repository.full_name : String(repository.id), page },
    });
  }

  getProjectDeployments(projectId: string) {
    return this.http.get<DeploymentList>(`/projects/${encodeURIComponent(projectId)}/deployments`, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getProjectMembers(projectId: string) {
    return this.http.get<MemberList>(`/projects/${encodeURIComponent(projectId)}/members`, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getSnapshot(projectId: string, snapshotId: number) {
    return this.http.get<Snapshot>(`/projects/${encodeURIComponent(projectId)}/snapshots/${snapshotId}`, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  getSnapshots(projectId: string, offset = 0) {
    return this.http.get<SnapshotList>(`/projects/${encodeURIComponent(projectId)}/snapshots`, {
      headers: { Authorization: `Bearer ${this.token()}` },
      params: { limit: 50, offset },
    });
  }

  getSnapshotTree(projectId: number, snapshotId: number, path = '') {
    return this.http.get<SnapshotTree>(`/projects/${projectId}/snapshots/${snapshotId}/tree`, {
      headers: { Authorization: `Bearer ${this.token()}` },
      params: { path },
    });
  }

  getSnapshotFile(projectId: number, snapshotId: number, path: string) {
    return this.http.get<SnapshotFile>(`/projects/${projectId}/snapshots/${snapshotId}/file`, {
      headers: { Authorization: `Bearer ${this.token()}` },
      params: { path },
    });
  }

  saveSnapshot(projectId: number, baseSnapshotId: number, files: { path: string; content: string }[], operations: SnapshotOperation[] = []) {
    return this.http.post<Snapshot>(`/projects/${projectId}/snapshots`, {
      base_snapshot_id: baseSnapshotId, files, ...(operations.length ? { operations } : {}),
    }, { headers: { Authorization: `Bearer ${this.token()}` } });
  }

  markAllNotificationsAsRead() {
    return this.http.post<void>('/users/read_notifications', null, {
      headers: { Authorization: `Bearer ${this.token()}` },
    });
  }

  clearSession() {
    sessionStorage.removeItem('authToken');
    sessionStorage.removeItem('authUsername');
    this.token.set(null);
    this.username.set('USERNAME');
  }
}
