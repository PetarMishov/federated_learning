import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { provideRouter, Router } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { Home } from './home';

const organizations = [
  { id: 1, name: 'Central Hospital', owner_user_id: 1 },
  { id: 2, name: 'Research Lab', owner_user_id: 1 },
];

describe('Home organizations', () => {
  beforeEach(() => {
    sessionStorage.clear();
    TestBed.configureTestingModule({
      imports: [Home],
      providers: [provideHttpClient(), provideHttpClientTesting(), provideRouter([])],
    });
  });

  afterEach(() => {
    TestBed.inject(HttpTestingController).verify();
    sessionStorage.clear();
  });

  it('loads with an existing session and handles an empty membership list', async () => {
    sessionStorage.setItem('authToken', 'saved-token');
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    const request = TestBed.inject(HttpTestingController).expectOne('/users/organizations');
    expect(request.request.headers.get('Authorization')).toBe('Bearer saved-token');
    request.flush({ organizations: [] });
    const notifications = TestBed.inject(HttpTestingController).expectOne('/users/notifications');
    expect(notifications.request.headers.get('Authorization')).toBe('Bearer saved-token');
    notifications.flush({ notifications: [] });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('You are not a member of any organizations yet.');
    expect(fixture.nativeElement.textContent).toContain('You have no notifications yet.');
  });

  it('clears an expired session and asks for sign-in again', async () => {
    sessionStorage.setItem('authToken', 'expired-token');
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    TestBed.inject(HttpTestingController).expectOne('/users/notifications').flush({ notifications: [] });
    TestBed.inject(HttpTestingController).expectOne('/users/organizations')
      .flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });

  it('offers a retry after a server error', async () => {
    sessionStorage.setItem('authToken', 'saved-token');
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/users/notifications').flush({ notifications: [] });
    http.expectOne('/users/organizations').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    fixture.nativeElement.querySelector('button').click();
    http.expectOne('/users/organizations').flush({ organizations });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Central Hospital');
  });

  it('retries notifications independently and displays read status', async () => {
    sessionStorage.setItem('authToken', 'saved-token');
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/users/organizations').flush({ organizations });
    http.expectOne('/users/notifications').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Central Hospital');
    fixture.nativeElement.querySelector('.notifications-panel button').click();
    http.expectNone('/users/organizations');
    http.expectOne('/users/notifications').flush({ notifications: [
      { id: 1, title: 'Welcome', is_read: false, created_at: Date.UTC(2026, 9, 4, 12, 30) },
      { id: 2, title: 'Organizations ready', is_read: true, created_at: Date.UTC(2026, 9, 3, 9) },
    ] });
    await fixture.whenStable();
    const items = fixture.nativeElement.querySelectorAll('.notification-list li');
    expect(items[0].textContent).toContain('Welcome');
    expect(items[0].textContent).toContain('Unread');
    expect(items[0].classList.contains('unread')).toBe(true);
    expect(items[1].textContent).toContain('Read');
    expect(items[1].classList.contains('unread')).toBe(false);
    expect(items[0].querySelector('time').getAttribute('datetime')).toBe('2026-10-04T12:30:00.000Z');
    expect(items[0].querySelector('time').textContent).toContain('2026');
    fixture.nativeElement.querySelector('.notifications-header button').click();
    http.expectNone('/users/notifications');
    expect(items[0].classList.contains('unread')).toBe(true);
  });

  it('clears the session when notifications reject authentication', async () => {
    sessionStorage.setItem('authToken', 'expired-token');
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
    const http = TestBed.inject(HttpTestingController);
    http.expectOne('/users/organizations').flush({ organizations: [] });
    http.expectOne('/users/notifications').flush('Unauthorized', { status: 401, statusText: 'Unauthorized' });
    await fixture.whenStable();
    expect(sessionStorage.getItem('authToken')).toBeNull();
    expect(navigate).toHaveBeenCalledWith('/login');
  });
});
