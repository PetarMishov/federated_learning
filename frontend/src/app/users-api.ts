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
  is_read: boolean;
  created_at: number;
}

export interface NotificationList {
  notifications: Notification[];
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
