import { HttpErrorResponse } from '@angular/common/http';
import { DatePipe } from '@angular/common';
import { Component, DestroyRef, OnInit, inject, signal } from '@angular/core';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { Router, RouterLink } from '@angular/router';
import { finalize } from 'rxjs';
import { Notification, Organization, UsersApi } from '../users-api';

@Component({
  selector: 'app-home',
  imports: [DatePipe, RouterLink],
  templateUrl: './home.html',
  styleUrl: './home.css',
})
export class Home implements OnInit {
  protected readonly api = inject(UsersApi);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly organizations = signal<Organization[]>([]);
  protected readonly loading = signal(false);
  protected readonly error = signal('');
  protected readonly notifications = signal<Notification[]>([]);
  protected readonly notificationsLoading = signal(false);
  protected readonly notificationsError = signal('');
  protected readonly markingNotificationsRead = signal(false);
  protected readonly markNotificationsError = signal('');
  private readonly router = inject(Router);

  ngOnInit() {
    if (this.api.token()) {
      this.loadOrganizations();
      this.loadNotifications();
    }
  }

  protected loadOrganizations() {
    if (this.loading()) return;
    this.loading.set(true);
    this.error.set('');
    this.api.getOrganizations().pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.loading.set(false)),
    ).subscribe({
      next: (result) => this.organizations.set(result.organizations),
      error: (error: HttpErrorResponse) => this.handleError(error),
    });
  }

  private handleError(error: HttpErrorResponse) {
    this.organizations.set([]);
    if (error.status === 401) {
      this.api.clearSession();
      void this.router.navigateByUrl('/login');
    } else {
      this.error.set('Could not connect or load organizations. Please try again.');
    }
  }

  protected loadNotifications() {
    if (this.notificationsLoading()) return;
    this.notificationsLoading.set(true);
    this.notificationsError.set('');
    this.api.getNotifications().pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.notificationsLoading.set(false)),
    ).subscribe({
      next: (result) => this.notifications.set(result.notifications),
      error: (error: HttpErrorResponse) => {
        this.notifications.set([]);
        if (error.status === 401) {
          this.api.clearSession();
          void this.router.navigateByUrl('/login');
        } else {
          this.notificationsError.set('Could not connect or load notifications. Please try again.');
        }
      },
    });
  }

  protected markAllNotificationsAsRead() {
    if (!this.api.token() || this.notificationsLoading() || this.markingNotificationsRead()) return;
    this.markingNotificationsRead.set(true);
    this.markNotificationsError.set('');
    this.api.markAllNotificationsAsRead().pipe(
      takeUntilDestroyed(this.destroyRef),
      finalize(() => this.markingNotificationsRead.set(false)),
    ).subscribe({
      next: () => this.loadNotifications(),
      error: (error: HttpErrorResponse) => {
        if (error.status === 401) {
          this.api.clearSession();
          void this.router.navigateByUrl('/login');
        } else {
          this.markNotificationsError.set('Could not mark notifications as read. Please try again.');
        }
      },
    });
  }
}
