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
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('You are not a member of any organizations yet.');
  });

  it('clears an expired session and asks for sign-in again', async () => {
    sessionStorage.setItem('authToken', 'expired-token');
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const fixture = TestBed.createComponent(Home);
    fixture.detectChanges();
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
    http.expectOne('/users/organizations').flush('Unavailable', { status: 500, statusText: 'Error' });
    await fixture.whenStable();
    fixture.nativeElement.querySelector('button').click();
    http.expectOne('/users/organizations').flush({ organizations });
    await fixture.whenStable();
    expect(fixture.nativeElement.textContent).toContain('Central Hospital');
  });
});
