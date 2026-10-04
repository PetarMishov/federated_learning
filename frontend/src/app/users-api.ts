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

  getOrganizations() {
    return this.http.get<OrganizationList>('/users/organizations', {
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
